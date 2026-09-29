// SPDX-License-Identifier: MIT OR Apache-2.0
//! `dream.vfs.VFS`: queries, mutation, materialization, and reports.

use std::rc::Rc;

use l3i::{
    Error, Result,
    bind::{Call, StackResults},
    extension::{ExtensionDescriptor, TagPolicy, UserdataBuilder},
    options::Options,
    sequence::Sequence,
    stack::{Scope, ValueView},
    userdata::{Owned, Userdata, push_owned},
    value::Table,
};

#[cfg(feature = "lua-write")]
use super::WriteRootGrant;
use super::{
    ConflictIndexHandle, LayerIndexHandle, Vfs, VfsFileHandle, VfsProviderHandle, class_function,
    frozen_class_table, host_path, io_error, paths_from_array, paths_from_table, regex_error,
    reports, types,
    views::{self, Entries, ProviderRecords, Providers},
    with_key,
};
use crate::{CollapseOptions, VFS};

/// The constructors' option table.
#[cfg(feature = "lua-write")]
const CONSTRUCTOR_OPTIONS: &str = "{ archives: { string }?, writeRoot: string? }?";
#[cfg(not(feature = "lua-write"))]
const CONSTRUCTOR_OPTIONS: &str = "{ archives: { string }? }?";

/// The `VFS` class table's type.
pub(super) fn class_type() -> String {
    format!(
        "{{ new: () -> dream_vfs_VFS, \
        fromDirectories: (dirs: {{ string }}, options: {CONSTRUCTOR_OPTIONS}) -> dream_vfs_VFS, \
        fromDirectoriesWithConflictIndex: (dirs: {{ string }}, options: {CONSTRUCTOR_OPTIONS}) -> (dream_vfs_VFS, dream_vfs_ConflictIndex), \
        fromDirectoriesWithLayerIndex: (dirs: {{ string }}, options: {CONSTRUCTOR_OPTIONS}) -> (dream_vfs_VFS, dream_vfs_LayerIndex) }}"
    )
}

/// What the constructors' option table says: `{ archives = { string }?, writeRoot = string? }?`.
#[derive(Default)]
struct Constructor {
    archives: Option<Vec<String>>,
    #[cfg(feature = "lua-write")]
    write_root: Option<std::path::PathBuf>,
}

impl Constructor {
    fn read(scope: &impl Scope, options: Option<ValueView<'_>>, context: &str) -> Result<Self> {
        let Some(options) = options.filter(|view| !view.is_nil()) else {
            return Ok(Constructor::default());
        };
        Options::read(scope, options, context, |o| {
            let archives_path = format!("{}.archives", o.context());
            let archives = o.optional_table("archives", |frame, archives| {
                let paths = paths_from_table(frame, &archives, &archives_path)?;
                Ok(paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect())
            })?;
            Ok(Constructor {
                archives,
                #[cfg(feature = "lua-write")]
                write_root: o.optional_bytes("writeRoot", |root| Ok(host_path(root)))?,
            })
        })
    }

    fn archives(&self) -> Option<Vec<&str>> {
        self.archives
            .as_ref()
            .map(|list| list.iter().map(String::as_str).collect())
    }

    /// The handle over `vfs`, with the write root when one was given and the grant allows it.
    fn handle(
        self,
        vfs: VFS,
        context: &str,
        #[cfg(feature = "lua-write")] grant: WriteRootGrant,
    ) -> Result<Vfs> {
        let handle = script_vfs(
            vfs,
            #[cfg(feature = "lua-write")]
            grant,
        );
        #[cfg(feature = "lua-write")]
        if let Some(root) = self.write_root {
            handle.check_write_root_grant(&format!("{context}.writeRoot"))?;
            handle.set_write_root(Some(root)).map_err(io_error)?;
        }
        #[cfg(not(feature = "lua-write"))]
        let _ = context;
        Ok(handle)
    }
}

/// A handle over a VFS a script built, carrying the runtime's write root grant.
fn script_vfs(vfs: VFS, #[cfg(feature = "lua-write")] grant: WriteRootGrant) -> Vfs {
    let handle = Vfs::new(vfs);
    #[cfg(feature = "lua-write")]
    handle.set_write_root_grant(grant);
    handle
}

/// `{ allowCopying, extractArchives, useSymlinks }?`.
fn collapse_options(
    scope: &impl Scope,
    options: Option<ValueView<'_>>,
    context: &str,
) -> Result<CollapseOptions> {
    let mut collapse = CollapseOptions {
        allow_copying: false,
        extract_archives: false,
        use_symlinks: false,
    };
    let Some(options) = options.filter(|view| !view.is_nil()) else {
        return Ok(collapse);
    };
    Options::read(scope, options, context, |o| {
        collapse.allow_copying = o.or("allowCopying", false)?;
        collapse.extract_archives = o.or("extractArchives", false)?;
        collapse.use_symlinks = o.or("useSymlinks", false)?;
        Ok(())
    })?;
    Ok(collapse)
}

/// The `VFS` constructors of one runtime. With `lua-write` they carry `grant`, what the
/// runtime's policy says about scripts giving a VFS a write root.
pub(super) fn class_table(
    runtime: &l3i::Runtime,
    #[cfg(feature = "lua-write")] grant: WriteRootGrant,
) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(runtime, table, "VFS", "new", move || {
            Owned(script_vfs(
                VFS::new(),
                #[cfg(feature = "lua-write")]
                grant,
            ))
        })?;
        class_function(
            runtime,
            table,
            "VFS",
            "fromDirectories",
            move |call: &Call, dirs: ValueView, options: Option<ValueView>| {
                const CONTEXT: &str = "VFS.fromDirectories";
                let dirs = paths_from_array(call, dirs, "dirs")?;
                let options = Constructor::read(call, options, CONTEXT)?;
                let vfs = VFS::from_directories(dirs.iter(), options.archives());
                let handle = options.handle(
                    vfs,
                    CONTEXT,
                    #[cfg(feature = "lua-write")]
                    grant,
                )?;
                Ok::<_, Error>(Owned(handle))
            },
        )?;
        class_function(
            runtime,
            table,
            "VFS",
            "fromDirectoriesWithConflictIndex",
            move |call: &Call, dirs: ValueView, options: Option<ValueView>| {
                const CONTEXT: &str = "VFS.fromDirectoriesWithConflictIndex";
                let dirs = paths_from_array(call, dirs, "dirs")?;
                let options = Constructor::read(call, options, CONTEXT)?;
                let (vfs, conflicts) =
                    VFS::from_directories_with_conflict_index(dirs.iter(), options.archives());
                let handle = options.handle(
                    vfs,
                    CONTEXT,
                    #[cfg(feature = "lua-write")]
                    grant,
                )?;
                push_owned(call, handle)?;
                push_owned(call, ConflictIndexHandle(conflicts))?;
                Ok::<_, Error>(StackResults)
            },
        )?;
        class_function(
            runtime,
            table,
            "VFS",
            "fromDirectoriesWithLayerIndex",
            move |call: &Call, dirs: ValueView, options: Option<ValueView>| {
                const CONTEXT: &str = "VFS.fromDirectoriesWithLayerIndex";
                let dirs = paths_from_array(call, dirs, "dirs")?;
                let options = Constructor::read(call, options, CONTEXT)?;
                let (vfs, layer) =
                    VFS::from_directories_with_layer_index(dirs.iter(), options.archives());
                let handle = options.handle(
                    vfs,
                    CONTEXT,
                    #[cfg(feature = "lua-write")]
                    grant,
                )?;
                push_owned(call, handle)?;
                push_owned(call, LayerIndexHandle(layer))?;
                Ok::<_, Error>(StackResults)
            },
        )
    })
}

pub(super) fn describe(d: &mut ExtensionDescriptor) {
    let mut vfs = d.userdata::<Vfs>(Vfs::NAME);
    vfs.tag(TagPolicy::Preferred)
        .doc("A virtual file system: provider stacks low to high priority and the cached resolved winner per key.");
    describe_queries(&mut vfs);
    describe_mutation(&mut vfs);
    describe_materialization(&mut vfs);
    describe_reports(&mut vfs);
    describe_archives_and_plans(&mut vfs);
    #[cfg(feature = "lua-write")]
    super::write::describe_vfs_writes(&mut vfs);
    vfs.metamethod("__tostring", |v: &Vfs| {
        v.with(|vfs| format!("dream.vfs.VFS({} files)", vfs.len()))
    });
}

fn describe_queries(vfs: &mut UserdataBuilder<'_, Vfs>) {
    vfs.method("len", |v: &Vfs| v.with(VFS::len).map(|len| len as f64))
        .signature("(self): number");
    vfs.method("isEmpty", |v: &Vfs| v.with(VFS::is_empty))
        .signature("(self): boolean");
    vfs.method("keys", |v: &Vfs, call: &Call| {
        views::push_keys(call, v.sorted_keys()?).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self): dream_vfs_Keys")
    .doc("Every resolved key, sorted, as a sequence view; sorted once and shared until the next mutation.");
    vfs.method("entries", |v: &Vfs, call: &Call| {
        let entries = v.with(|vfs| Entries::cloned(vfs.iter()))?;
        views::push_entries(call, entries).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self): dream_vfs_Entries")
    .doc("Every resolved entry sorted by key, as a sequence view of { key, file } rows.");
    vfs.method("getFile", |v: &Vfs, path: &[u8]| {
        v.with(|vfs| with_key(path, |key| vfs.get_file_normalized(key).cloned()))
            .map(|file| file.map(VfsFileHandle::new).map(Owned))
    })
    .signature("(self, path: string): dream_vfs_VfsFile?")
    .doc("The winning file for a path in any spelling, or nil.");
    vfs.method("contains", |v: &Vfs, path: &[u8]| {
        v.with(|vfs| with_key(path, |key| vfs.contains_normalized(key)))
    })
    .signature("(self, path: string): boolean");
    vfs.method(
        "findByRegex",
        |v: &Vfs, call: &Call, pattern: &str, relative: Option<bool>| {
            let tree = v
                .with(|vfs| vfs.find_by_regex(pattern, relative.unwrap_or(true)))?
                .map_err(regex_error)?;
            views::push_tree(call, tree).map(drop)?;
            Ok::<_, Error>(StackResults)
        },
    )
    .signature("(self, pattern: string, relative: boolean?): dream_vfs_Tree")
    .doc("The files whose key matches a case-insensitive regex, as a tree.");
    vfs.method("remaining", |v: &Vfs, call: &Call, filter_path: &[u8], replacements_only: bool, all_dirs: ValueView, relative: Option<bool>| {
        let all_dirs = paths_from_array(call, all_dirs, "allDirs")?;
        let tree = v.with(|vfs| vfs.remaining(&host_path(filter_path), replacements_only, &all_dirs, relative.unwrap_or(true)))?;
        views::push_tree(call, tree).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self, filterPath: string, replacementsOnly: boolean, allDirs: { string }, relative: boolean?): dream_vfs_Tree");
    vfs.method("pathsMatching", |v: &Vfs, call: &Call, substring: &[u8]| {
        let entries = v.with(|vfs| Entries::cloned(vfs.paths_matching_bytes(substring)))?;
        views::push_entries(call, entries).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self, substring: string): dream_vfs_Entries")
    .doc("Entries whose key contains the substring (folded like a path), sorted by key.");
    vfs.method("pathsWith", |v: &Vfs, call: &Call, prefix: &[u8]| {
        let entries = v.with(|vfs| Entries::cloned(vfs.paths_with(prefix)))?;
        views::push_entries(call, entries).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self, prefix: string): dream_vfs_Entries")
    .doc("Entries at or under a directory, matched on component boundaries, sorted by key.");
}

fn describe_mutation(vfs: &mut UserdataBuilder<'_, Vfs>) {
    vfs.method("setWinnerLooseFile", |v: &Vfs, key: &[u8], path: &[u8]| {
        v.with_mut(|vfs| vfs.set_winner_loose_file(key, host_path(path))).map(|file| file.map(VfsFileHandle::new).map(Owned))
    })
    .signature("(self, key: string, physicalPath: string): dream_vfs_VfsFile?")
    .doc("Replaces the whole provider stack for key with one loose winner; returns the previous winner.");
    vfs.method(
        "setWinnerFile",
        |v: &Vfs, key: &[u8], file: &VfsFileHandle| {
            v.with_mut(|vfs| vfs.set_winner_file(key, file.file.clone()))
                .map(|file| file.map(VfsFileHandle::new).map(Owned))
        },
    )
    .signature("(self, key: string, file: dream_vfs_VfsFile): dream_vfs_VfsFile?");
    vfs.method("pushDirectory", |v: &Vfs, root: &[u8]| {
        v.with_mut(|vfs| vfs.push_directory(host_path(root)))?
            .map_err(io_error)
    })
    .signature("(self, path: string)")
    .doc("Inserts every loose file under the directory as a higher-priority provider.");
    vfs.method(
        "pushProvider",
        |v: &Vfs, key: &[u8], provider: &VfsProviderHandle| {
            v.with_mut(|vfs| vfs.push_provider(key, provider.0.clone()))
        },
    )
    .signature("(self, key: string, provider: dream_vfs_VfsProvider): boolean")
    .doc("Inserts a provider at highest priority; false for an unsafe key.");
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    vfs.method("pushArchive", |v: &Vfs, archive: &[u8]| {
        v.with_mut(|vfs| vfs.push_archive(host_path(archive)))
    })
    .signature("(self, path: string): boolean")
    .doc("Inserts every entry of an archive as the newest, highest-priority source.");
    vfs.method("removeWinner", |v: &Vfs, key: &[u8]| {
        v.with_mut(|vfs| vfs.remove_winner(key))
            .map(|provider| provider.map(VfsProviderHandle).map(Owned))
    })
    .signature("(self, key: string): dream_vfs_VfsProvider?")
    .doc("Removes only the current winner, revealing the next provider.");
    vfs.method("removeResolvedFile", |v: &Vfs, key: &[u8]| {
        v.with_mut(|vfs| vfs.remove_resolved_file(key))
            .map(|file| file.map(VfsFileHandle::new).map(Owned))
    })
    .signature("(self, key: string): dream_vfs_VfsFile?")
    .doc("Removes the key and its whole provider stack.");
    vfs.method(
        "removeProviderPrefix",
        |v: &Vfs, call: &Call, prefix: &[u8]| {
            let removed = v.with_mut(|vfs| vfs.remove_provider_prefix(prefix))?;
            reports::removed_providers(call, &removed)
        },
    )
    .signature(format!(
        "(self, prefix: string): {}",
        types::REMOVED_PROVIDERS
    ));
    vfs.method(
        "removeResolvedPrefix",
        |v: &Vfs, call: &Call, prefix: &[u8]| {
            let removed = v.with_mut(|vfs| vfs.remove_resolved_prefix(prefix))?;
            reports::removed_files(call, &removed)
        },
    )
    .signature(format!("(self, prefix: string): {}", types::REMOVED_FILES));
    vfs.method(
        "removeProvider",
        |v: &Vfs, call: &Call, key: &[u8], source: &[u8]| {
            let removed = v.with_mut(|vfs| vfs.remove_provider(key, &host_path(source)))?;
            reports::providers_list(call, &removed)
        },
    )
    .signature(format!(
        "(self, key: string, sourcePath: string): {{ {} }}",
        types::PROVIDER
    ))
    .doc("Removes every provider of key whose source path equals sourcePath lexically.");
    vfs.method("removeSource", |v: &Vfs, call: &Call, source: &[u8]| {
        let removed = v.with_mut(|vfs| vfs.remove_source(&host_path(source)))?;
        reports::removed_providers(call, &removed)
    })
    .signature(format!(
        "(self, sourcePath: string): {}",
        types::REMOVED_PROVIDERS
    ));
    vfs.method(
        "removeResolvedMatchingGlob",
        |v: &Vfs, call: &Call, glob: &str| {
            let removed = v.with_mut(|vfs| vfs.remove_resolved_matching_glob(glob))?;
            reports::removed_files(call, &removed)
        },
    )
    .signature(format!("(self, glob: string): {}", types::REMOVED_FILES));
}

fn describe_materialization(vfs: &mut UserdataBuilder<'_, Vfs>) {
    vfs.method("tree", |v: &Vfs, call: &Call, relative: Option<bool>| {
        let tree = v.with(|vfs| vfs.tree(relative.unwrap_or(true)))?;
        views::push_tree(call, tree).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self, relative: boolean?): dream_vfs_Tree")
    .doc("The sorted directory tree as a stream of { dir, file } rows; tree:toTable() is the nested shape.");
    vfs.method("display", |v: &Vfs, relative: Option<bool>| {
        v.with(|vfs| vfs.display_filtered(relative.unwrap_or(true), |_, _| true))
    })
    .signature("(self, relative: boolean?): string");
    vfs.method(
        "dumpToDirectory",
        |v: &Vfs, call: &Call, dir: &[u8], use_hardlinks: bool| {
            let report = v
                .with(|vfs| vfs.dump_to_directory(&host_path(dir), use_hardlinks))?
                .map_err(io_error)?;
            let skipped = reports::materialization_skips(call, &report.skipped)?;
            call.push(&(report.written as f64))?;
            call.push(&skipped)?;
            Ok::<_, Error>(StackResults)
        },
    )
    .signature(format!(
        "(self, dir: string, useHardlinks: boolean): (number, {})",
        types::SKIPPED
    ))
    .doc("Writes every winner into dir; returns how many were written and the ones left out.");
    vfs.method(
        "collapseInto",
        |v: &Vfs, call: &Call, dest: &[u8], options: Option<ValueView>| {
            let options = collapse_options(call, options, "vfs:collapseInto")?;
            let report = v
                .with(|vfs| vfs.collapse_into(&host_path(dest), &options))?
                .map_err(io_error)?;
            reports::materialization_skips(call, &report.skipped)
        },
    )
    .signature(format!(
        "(self, dest: string, options: {}): {}",
        types::COLLAPSE_OPTIONS,
        types::SKIPPED
    ))
    .doc("Writes the VFS into dest as one directory; returns the winners left out.");
    vfs.method("extractFile", |v: &Vfs, path: &[u8], dest: &[u8]| {
        v.with(|vfs| vfs.extract_file(&host_path(path), &host_path(dest)))?
            .map(|written| written.map(|path| super::handles::path_string(&path)))
            .map_err(io_error)
    })
    .signature("(self, vfsPath: string, destDir: string): string?");
    vfs.method("diffDirectory", |v: &Vfs, call: &Call, dir: &[u8]| {
        v.with(|vfs| reports::directory_diff(call, &vfs.diff_directory(host_path(dir))))?
    })
    .signature(format!("(self, dir: string): {}", types::DIRECTORY_DIFF));
}

fn describe_reports(vfs: &mut UserdataBuilder<'_, Vfs>) {
    vfs.method("providerRecordsFor", |v: &Vfs, call: &Call, path: &[u8]| {
        let records: Rc<[_]> = v.with(|vfs| vfs.provider_records_for(path))?.into();
        Sequence::push(call, ProviderRecords(records)).map(drop)?;
        Ok::<_, Error>(StackResults)
    })
    .signature("(self, path: string): dream_vfs_ProviderRecords")
    .doc("Provider report rows for a key, low to high priority, as a sequence view.");
    vfs.method("explain", |v: &Vfs, call: &Call, path: &[u8]| {
        match v.with(|vfs| vfs.explain(path))? {
            Some(report) => reports::explain(call, &report).map(Some),
            None => Ok(None),
        }
    })
    .signature(format!("(self, path: string): {}?", types::EXPLAIN))
    .doc("Why a path resolves to its winner: the winner and the providers it overrides.");
    vfs.method("providersFor", |v: &Vfs, call: &Call, key: &[u8]| {
        let providers: Option<Rc<[_]>> = v.with(|vfs| vfs.providers_for(key).map(|providers| providers.cloned().collect()))?;
        match providers {
            Some(providers) => {
                Sequence::push(call, Providers(providers)).map(drop)?;
            }
            None => {
                call.push(&())?;
            }
        }
        Ok::<_, Error>(StackResults)
    })
    .signature("(self, key: string): dream_vfs_Providers?")
    .doc("Every provider for a key, low to high priority, as a sequence view of { source, file } rows; nil for an unknown key.");
    vfs.method(
        "duplicates",
        |v: &Vfs, call: &Call, pattern: Option<&str>| {
            let report = match pattern {
                Some(pattern) => v
                    .with(|vfs| vfs.duplicates_matching_regex(pattern))?
                    .map_err(regex_error)?,
                None => v.with(VFS::duplicates)?,
            };
            reports::duplicates(call, &report)
        },
    )
    .signature(format!("(self, pattern: string?): {}", types::DUPLICATES));
}

fn describe_archives_and_plans(vfs: &mut UserdataBuilder<'_, Vfs>) {
    vfs.method("archives", |v: &Vfs, call: &Call| {
        v.with(|vfs| reports::archive_infos(call, &vfs.archives()))?
    })
    .signature(format!("(self): {{ {} }}", types::ARCHIVE_INFO));
    vfs.method("archiveEntries", |v: &Vfs, call: &Call, archive: &[u8]| {
        v.with(|vfs| reports::archive_entries(call, &vfs.archive_entries(host_path(archive))))?
    })
    .signature(format!(
        "(self, archive: string): {{ {} }}",
        types::ARCHIVE_ENTRY
    ));
    vfs.method(
        "filesFromArchive",
        |v: &Vfs, call: &Call, archive: &[u8]| {
            let keys = v.with(|vfs| {
                views::key_blob_of_paths(
                    vfs.files_from_archive(host_path(archive))
                        .iter()
                        .map(std::path::PathBuf::as_path),
                )
            })?;
            views::push_keys(call, keys).map(drop)?;
            Ok::<_, Error>(StackResults)
        },
    )
    .signature("(self, archive: string): dream_vfs_Keys");
    vfs.method("sourceContributions", |v: &Vfs, call: &Call| {
        v.with(|vfs| reports::source_contributions(call, &vfs.source_contributions()))?
    })
    .signature(format!("(self): {}", types::CONTRIBUTIONS));
    vfs.method(
        "materializationPlan",
        |v: &Vfs, call: &Call, dest: &[u8], options: Option<ValueView>| {
            let options = collapse_options(call, options, "vfs:materializationPlan")?;
            v.with(|vfs| {
                reports::materialization_plan(
                    call,
                    &vfs.materialization_plan(host_path(dest), &options),
                )
            })?
        },
    )
    .signature(format!(
        "(self, dest: string, options: {}): {}",
        types::COLLAPSE_OPTIONS,
        types::PLAN
    ));
    vfs.method("layerIndex", |v: &Vfs| {
        v.with(|vfs| Owned(LayerIndexHandle(vfs.layer_index().clone())))
    })
    .signature("(self): dream_vfs_LayerIndex");
    #[cfg(feature = "serialize")]
    vfs.method(
        "serializeTree",
        |v: &Vfs, relative: Option<bool>, format: &str| {
            let format = super::serialize::format(format)?;
            v.with(|vfs| VFS::serialize_from_tree(&vfs.tree(relative.unwrap_or(true)), format))?
                .map_err(io_error)
        },
    )
    .signature("(self, relative: boolean?, format: string): string");
}
