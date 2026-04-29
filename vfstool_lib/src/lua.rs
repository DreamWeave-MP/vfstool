// SPDX-License-Identifier: GPL-3.0-only
//! Embedded Lua bindings for the promoted stable `vfstool_lib` API.
//!
//! This module intentionally exposes only the stable top-level API surface and methods on those
//! stable types. It does not bind `experimental`, low-level archive internals, or implementation
//! details that merely happen to be public Rust modules.

use crate::analysis::{ProvenanceChain, ProviderRecord};
use crate::{
    ArchiveHashMode, AssetClass, CollapseOptions, ConflictIndex, ConflictSourceEntry,
    ConflictsReport, DiffReport, DirectoryDiff, DriftEntry, DriftKind, DriftReport, DuplicateEntry,
    DuplicateReport, ExplainReport, LayerIndex, LayerProvider, MaterializationAction,
    MaterializationIssue, MaterializationPlan, MetadataSnapshot, NormalizedPath,
    SemanticConflictReport, SemanticDelta, SemanticOpts, SemanticProvider, SemanticRelation,
    ShadowedReport, ShadowedSource, Snapshot, SourceContribution, SourceContributionReport,
    SourceKind, SourceMeta, VFS, VfsFile, VfsLock, VfsLockEntry, VfsProvider, VfsProviderRecord,
    analyze_pair, changed_files, changed_files_metadata, normalize_host_path_in_place,
    path_glob_matches, paths::key_to_path_buf_lossy, run_finalize, run_finalize_tracked, run_setup,
    run_setup_tracked, snapshot_directory, snapshot_directory_metadata, source_glob_matches,
};
#[cfg(feature = "serialize")]
use crate::{SerializeType, serialize_value};
use mlua::{
    AnyUserData, Error as LuaError, Lua, Result as LuaResult, Table, UserData, UserDataMethods,
    Value, Variadic,
};
use std::path::{Path, PathBuf};

/// Return a Lua module table containing the embedded `vfstool` API.
///
/// The function does not register globals. Use [`register`] when that is desired.
///
/// # Errors
///
/// Returns an `mlua` error if creating the module table or functions fails.
pub fn open(lua: &Lua) -> LuaResult<Table> {
    let module = lua.create_table()?;

    module.set(
        "normalize_host_path",
        lua.create_function(|lua, path| Ok(lua_normalize_host_path(lua, path)))?,
    )?;
    module.set(
        "normalize_host_path_in_place",
        lua.create_function(|lua, path| Ok(lua_normalize_host_path(lua, path)))?,
    )?;
    module.set(
        "path_glob_matches",
        lua.create_function(|_, (glob, path): (String, String)| {
            Ok(path_glob_matches(&glob, Path::new(&path)))
        })?,
    )?;
    module.set(
        "source_glob_matches",
        lua.create_function(|_, (glob, source): (String, String)| {
            Ok(source_glob_matches(&glob, &PathBuf::from(source)))
        })?,
    )?;
    module.set("analyze_pair", lua.create_function(lua_analyze_pair)?)?;
    module.set("run_setup", lua.create_function(lua_run_setup)?)?;
    module.set(
        "run_setup_tracked",
        lua.create_function(lua_run_setup_tracked)?,
    )?;
    module.set("run_finalize", lua.create_function(lua_run_finalize)?)?;
    module.set(
        "run_finalize_tracked",
        lua.create_function(lua_run_finalize_tracked)?,
    )?;
    module.set(
        "snapshot_directory",
        lua.create_function(|_, dir: String| {
            snapshot_directory(Path::new(&dir))
                .map(LuaSnapshot)
                .map_err(LuaError::external)
        })?,
    )?;
    module.set(
        "snapshot_directory_metadata",
        lua.create_function(|_, dir: String| {
            snapshot_directory_metadata(Path::new(&dir))
                .map(LuaMetadataSnapshot)
                .map_err(LuaError::external)
        })?,
    )?;
    module.set("changed_files", lua.create_function(lua_changed_files)?)?;
    module.set(
        "changed_files_metadata",
        lua.create_function(lua_changed_files_metadata)?,
    )?;

    #[cfg(feature = "serialize")]
    module.set("serialize", lua.create_function(lua_serialize)?)?;

    module.set("VFS", vfs_class(lua)?)?;
    module.set("VfsFile", vfs_file_class(lua)?)?;
    module.set("VfsProvider", vfs_provider_class(lua)?)?;
    module.set("LayerIndex", layer_index_class(lua)?)?;
    module.set("ConflictIndex", conflict_index_class(lua)?)?;

    Ok(module)
}

/// Register the embedded module as global `vfstool`.
///
/// # Errors
///
/// Returns an `mlua` error if creating or installing the module table fails.
pub fn register(lua: &Lua) -> LuaResult<()> {
    let module = open(lua)?;
    lua.globals().set("vfstool", module)
}

struct LuaVfs(VFS);

#[derive(Debug, Clone)]
struct LuaVfsFile(VfsFile);

#[derive(Debug, Clone)]
struct LuaLayerIndex(LayerIndex);

struct LuaConflictIndex(ConflictIndex);

#[derive(Debug)]
struct LuaSnapshot(Snapshot);

#[derive(Debug)]
struct LuaMetadataSnapshot(MetadataSnapshot);

#[derive(Debug, Clone)]
struct LuaVfsLock(VfsLock);

fn vfs_class(lua: &Lua) -> LuaResult<Table> {
    let class = lua.create_table()?;
    class.set("new", lua.create_function(|_, ()| Ok(LuaVfs(VFS::new())))?)?;
    class.set(
        "from_directories",
        lua.create_function(|_, (dirs, opts): (Table, Option<Table>)| {
            let dirs = pathbufs_from_sequence(&dirs)?;
            let archive_list = archive_list_from_opts(opts)?;
            let archive_refs = archive_list
                .as_ref()
                .map(|items| items.iter().map(String::as_str).collect::<Vec<_>>());
            Ok(LuaVfs(VFS::from_directories(dirs.iter(), archive_refs)))
        })?,
    )?;
    class.set(
        "from_directories_with_conflict_index",
        lua.create_function(|_, (dirs, opts): (Table, Option<Table>)| {
            let dirs = pathbufs_from_sequence(&dirs)?;
            let archive_list = archive_list_from_opts(opts)?;
            let archive_refs = archive_list
                .as_ref()
                .map(|items| items.iter().map(String::as_str).collect::<Vec<_>>());
            let (vfs, conflict) =
                VFS::from_directories_with_conflict_index(dirs.iter(), archive_refs);
            Ok((LuaVfs(vfs), LuaConflictIndex(conflict)))
        })?,
    )?;
    class.set(
        "from_directories_with_layer_index",
        lua.create_function(|_, (dirs, opts): (Table, Option<Table>)| {
            let dirs = pathbufs_from_sequence(&dirs)?;
            let archive_list = archive_list_from_opts(opts)?;
            let archive_refs = archive_list
                .as_ref()
                .map(|items| items.iter().map(String::as_str).collect::<Vec<_>>());
            let (vfs, layer) = VFS::from_directories_with_layer_index(dirs.iter(), archive_refs);
            Ok((LuaVfs(vfs), LuaLayerIndex(layer)))
        })?,
    )?;
    Ok(class)
}

fn vfs_file_class(lua: &Lua) -> LuaResult<Table> {
    let class = lua.create_table()?;
    class.set(
        "from",
        lua.create_function(|_, path: String| Ok(LuaVfsFile(VfsFile::from(path))))?,
    )?;
    Ok(class)
}

fn vfs_provider_class(lua: &Lua) -> LuaResult<Table> {
    let class = lua.create_table()?;
    class.set(
        "new",
        lua.create_function(|_, (source, file): (Table, AnyUserData)| {
            let file = file.borrow::<LuaVfsFile>()?;
            Ok(LuaVfsProvider(VfsProvider {
                source: source_meta_from_table(&source)?,
                file: file.0.clone(),
            }))
        })?,
    )?;
    Ok(class)
}

fn layer_index_class(lua: &Lua) -> LuaResult<Table> {
    let class = lua.create_table()?;
    class.set(
        "from_file_lists",
        lua.create_function(|_, sources: Table| {
            let mut rows = Vec::new();
            for row in sources.sequence_values::<Table>() {
                let row = row?;
                let source_table = row.get::<Table>("source")?;
                let source = source_meta_from_table(&source_table)?;
                let files = strings_from_sequence(&row.get::<Table>("files")?)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
                rows.push((source, files));
            }
            Ok(LuaLayerIndex(LayerIndex::from_file_lists(rows)))
        })?,
    )?;
    Ok(class)
}

fn conflict_index_class(lua: &Lua) -> LuaResult<Table> {
    let class = lua.create_table()?;
    class.set(
        "from_directories",
        lua.create_function(|_, dirs: Table| {
            let dirs = pathbufs_from_sequence(&dirs)?;
            Ok(LuaConflictIndex(ConflictIndex::from_directories(
                dirs.iter(),
            )))
        })?,
    )?;
    class.set(
        "from_file_lists",
        lua.create_function(|_, sources: Table| {
            let mut rows = Vec::new();
            for row in sources.sequence_values::<Table>() {
                let row = row?;
                rows.push((
                    PathBuf::from(row.get::<String>("source")?),
                    strings_from_sequence(&row.get::<Table>("files")?)?
                        .into_iter()
                        .map(PathBuf::from)
                        .collect(),
                ));
            }
            Ok(LuaConflictIndex(ConflictIndex::from_file_lists(rows)))
        })?,
    )?;
    class.set(
        "from_layer_index",
        lua.create_function(|_, layer: AnyUserData| {
            let layer = layer.borrow::<LuaLayerIndex>()?;
            Ok(LuaConflictIndex(ConflictIndex::from_layer_index(&layer.0)))
        })?,
    )?;
    Ok(class)
}

impl UserData for LuaVfs {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_vfs_query_methods(methods);
        add_vfs_mutation_methods(methods);
        add_vfs_materialization_methods(methods);
        add_vfs_report_methods(methods);
    }
}

fn add_vfs_query_methods<M: UserDataMethods<LuaVfs>>(methods: &mut M) {
    methods.add_method("len", |_, this, ()| Ok(this.0.iter().count()));
    methods.add_method("is_empty", |_, this, ()| Ok(this.0.iter().next().is_none()));
    methods.add_method("keys", |lua, this, ()| {
        normalized_paths_to_table(lua, this.0.iter().map(|(k, _)| k))
    });
    methods.add_method("entries", |lua, this, ()| {
        vfs_entries_to_table(lua, this.0.iter())
    });
    methods.add_method("get_file", |_, this, path: String| {
        Ok(this.0.get_file(&path).cloned().map(LuaVfsFile))
    });
    methods.add_method("contains", |_, this, path: String| {
        Ok(this.0.contains(Path::new(&path)))
    });
    methods.add_method(
        "find_by_regex",
        |lua, this, (pattern, relative): (String, Option<bool>)| {
            display_tree_to_table(
                lua,
                &this
                    .0
                    .find_by_regex(&pattern, relative.unwrap_or(true))
                    .map_err(LuaError::external)?,
            )
        },
    );
    methods.add_method(
        "remaining",
        |lua,
         this,
         (filter_path, replacements_only, all_dirs, relative): (
            String,
            bool,
            Table,
            Option<bool>,
        )| {
            let all_dirs = strings_from_sequence(&all_dirs)?
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            display_tree_to_table(
                lua,
                &this.0.remaining(
                    Path::new(&filter_path),
                    replacements_only,
                    &all_dirs,
                    relative.unwrap_or(true),
                ),
            )
        },
    );
    methods.add_method("paths_matching", |lua, this, substring: String| {
        vfs_entries_to_table(lua, this.0.paths_matching(substring))
    });
    methods.add_method("paths_with", |lua, this, prefix: String| {
        vfs_entries_to_table(lua, this.0.paths_with(&prefix))
    });
}

fn add_vfs_mutation_methods<M: UserDataMethods<LuaVfs>>(methods: &mut M) {
    methods.add_method_mut(
        "set_winner_loose_file",
        |_, this, (key, path): (String, String)| {
            Ok(this.0.set_winner_loose_file(&key, path).map(LuaVfsFile))
        },
    );
    methods.add_method_mut(
        "set_winner_file",
        |_, this, (key, file): (String, AnyUserData)| {
            let file = file.borrow::<LuaVfsFile>()?;
            Ok(this.0.set_winner_file(&key, file.0.clone()).map(LuaVfsFile))
        },
    );
    methods.add_method_mut("push_directory", |_, this, root: String| {
        this.0.push_directory(root).map_err(LuaError::external)
    });
    methods.add_method_mut(
        "push_provider",
        |_, this, (key, provider): (String, AnyUserData)| {
            let provider = provider.borrow::<LuaVfsProvider>()?;
            Ok(this.0.push_provider(&key, provider.0.clone()))
        },
    );
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    methods.add_method_mut("push_archive", |_, this, archive: String| {
        Ok(this.0.push_archive(archive))
    });
    methods.add_method_mut("remove_winner", |_, this, key: String| {
        Ok(this.0.remove_winner(&key).map(LuaVfsProvider))
    });
    methods.add_method_mut("remove_resolved_file", |_, this, key: String| {
        Ok(this.0.remove_resolved_file(&key).map(LuaVfsFile))
    });
    methods.add_method_mut("remove_provider_prefix", |lua, this, prefix: String| {
        removed_providers_to_table(lua, &this.0.remove_provider_prefix(&prefix))
    });
    methods.add_method_mut("remove_resolved_prefix", |lua, this, prefix: String| {
        removed_vfs_files_to_table(lua, this.0.remove_resolved_prefix(&prefix))
    });
    methods.add_method_mut(
        "remove_provider",
        |lua, this, (key, source): (String, String)| {
            mutable_providers_to_table(lua, &this.0.remove_provider(&key, Path::new(&source)))
        },
    );
    methods.add_method_mut("remove_source", |lua, this, source: String| {
        removed_providers_to_table(lua, &this.0.remove_source(Path::new(&source)))
    });
    methods.add_method_mut(
        "remove_resolved_matching_glob",
        |lua, this, glob: String| {
            removed_vfs_files_to_table(lua, this.0.remove_resolved_matching_glob(&glob))
        },
    );
}

fn add_vfs_materialization_methods<M: UserDataMethods<LuaVfs>>(methods: &mut M) {
    methods.add_method("tree", |lua, this, relative: Option<bool>| {
        display_tree_to_table(lua, &this.0.tree(relative.unwrap_or(true)))
    });
    methods.add_method("display", |_, this, relative: Option<bool>| {
        Ok(this
            .0
            .display_filtered(relative.unwrap_or(true), |_, _| true))
    });
    methods.add_method(
        "dump_to_directory",
        |_, this, (dir, use_hardlinks): (String, bool)| {
            this.0
                .dump_to_directory(Path::new(&dir), use_hardlinks)
                .map_err(LuaError::external)
        },
    );
    methods.add_method(
        "collapse_into",
        |_, this, (dest, opts): (String, Option<Table>)| {
            let opts = collapse_options_from_table(opts);
            this.0
                .collapse_into(Path::new(&dest), &opts)
                .map_err(LuaError::external)
        },
    );
    methods.add_method("extract_file", |_, this, (path, dest): (String, String)| {
        this.0
            .extract_file(Path::new(&path), Path::new(&dest))
            .map(|path| path.map(path_to_string))
            .map_err(LuaError::external)
    });
    methods.add_method("diff_directory", |lua, this, dir: String| {
        directory_diff_to_table(lua, &this.0.diff_directory(dir))
    });
}

fn add_vfs_report_methods<M: UserDataMethods<LuaVfs>>(methods: &mut M) {
    methods.add_method("provider_records_for", |lua, this, path: String| {
        provider_records_to_table(lua, &this.0.provider_records_for(&path))
    });
    methods.add_method("explain", |lua, this, path: String| {
        optional_explain_to_value(lua, this.0.explain(&path))
    });
    methods.add_method("providers_for", |lua, this, key: String| {
        match this.0.providers_for(&key) {
            Some(providers) => {
                let providers = providers.cloned().collect::<Vec<_>>();
                mutable_providers_to_value(lua, &providers)
            }
            None => Ok(Value::Nil),
        }
    });
    methods.add_method("duplicates", |lua, this, pattern: Option<String>| {
        let report = match pattern {
            Some(pattern) => this
                .0
                .duplicates_matching_regex(&pattern)
                .map_err(LuaError::external)?,
            None => this.0.duplicates(),
        };
        duplicate_report_to_table(lua, &report)
    });
    methods.add_method("archives", |lua, this, ()| {
        archive_infos_to_table(lua, &this.0.archives())
    });
    methods.add_method("archive_entries", |lua, this, archive: String| {
        archive_entries_to_table(lua, &this.0.archive_entries(archive))
    });
    methods.add_method("files_from_archive", |lua, this, archive: String| {
        paths_to_table(lua, this.0.files_from_archive(archive).iter())
    });
    methods.add_method("source_contributions", |lua, this, ()| {
        source_contribution_report_to_table(lua, &this.0.source_contributions())
    });
    methods.add_method(
        "materialization_plan",
        |lua, this, (dest, opts): (String, Option<Table>)| {
            let opts = collapse_options_from_table(opts);
            materialization_plan_to_table(lua, &this.0.materialization_plan(dest, &opts))
        },
    );
    methods.add_method("layer_index", |_, this, ()| {
        Ok(LuaLayerIndex(this.0.layer_index().clone()))
    });
    #[cfg(feature = "serialize")]
    methods.add_method(
        "serialize_tree",
        |_, this, (relative, format): (Option<bool>, String)| {
            crate::VFS::serialize_from_tree(
                &this.0.tree(relative.unwrap_or(true)),
                serialize_type(&format)?,
            )
            .map_err(LuaError::external)
        },
    );
}

impl UserData for LuaVfsFile {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_loose", |_, this, ()| Ok(this.0.is_loose()));
        methods.add_method("is_archive", |_, this, ()| Ok(this.0.is_archive()));
        methods.add_method("path", |_, this, ()| Ok(path_to_string(this.0.path())));
        methods.add_method("file_name", |_, this, ()| {
            Ok(this.0.file_name().map(|s| s.to_string_lossy().into_owned()))
        });
        methods.add_method("file_stem", |_, this, ()| {
            Ok(this.0.file_stem().map(|s| s.to_string_lossy().into_owned()))
        });
        methods.add_method("parent_archive_path", |_, this, ()| {
            Ok(this.0.parent_archive_path())
        });
        methods.add_method("parent_archive_name", |_, this, ()| {
            Ok(this.0.parent_archive_name())
        });
        methods.add_method("read_all", |_, this, ()| {
            let mut reader = this.0.open().map_err(LuaError::external)?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut reader, &mut bytes).map_err(LuaError::external)?;
            Ok(bytes)
        });
    }
}

#[derive(Debug, Clone)]
struct LuaVfsProvider(VfsProvider);

impl UserData for LuaVfsProvider {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("source", |lua, this, ()| {
            source_meta_to_table(lua, &this.0.source)
        });
        methods.add_method("file", |_, this, ()| Ok(LuaVfsFile(this.0.file.clone())));
    }
}

impl UserData for LuaLayerIndex {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("keys", |lua, this, ()| {
            normalized_paths_to_table(lua, this.0.keys().iter())
        });
        methods.add_method("sources", |lua, this, ()| {
            sources_to_table(lua, &this.0.sources)
        });
        methods.add_method("source_id_for_path", |_, this, path: String| {
            Ok(this
                .0
                .source_id_for_path(Path::new(&path))
                .map(crate::SourceId::as_index))
        });
        methods.add_method("source_by_id", |lua, this, id: usize| {
            match this.0.source_by_id(crate::SourceId::from_index(id)) {
                Some(source) => Ok(Value::Table(source_meta_to_table(lua, source)?)),
                None => Ok(Value::Nil),
            }
        });
        methods.add_method("sources_containing", |lua, this, path: String| {
            indices_to_table(lua, this.0.sources_containing(Path::new(&path)))
        });
        methods.add_method(
            "provider_original_path",
            |_, this, (source_index, path): (usize, String)| {
                Ok(this
                    .0
                    .provider_original_path(source_index, Path::new(&path))
                    .map(path_to_string_ref))
            },
        );
        methods.add_method("provider_chain", |lua, this, path: String| {
            layer_providers_to_table(lua, &this.0.provider_chain(Path::new(&path)))
        });
        methods.add_method("duplicate_keys", |lua, this, ()| {
            normalized_paths_to_table(lua, this.0.duplicate_keys().iter())
        });
        methods.add_method("source_contributions", |lua, this, ()| {
            source_contribution_report_to_table(lua, &this.0.source_contributions())
        });
        methods.add_method(
            "provenance",
            |lua, this, (vfs, path, with_hashes): (AnyUserData, String, bool)| {
                let vfs = vfs.borrow::<LuaVfs>()?;
                match this
                    .0
                    .provenance(&vfs.0, Path::new(&path), with_hashes)
                    .map_err(LuaError::external)?
                {
                    Some(chain) => Ok(Value::Table(provenance_chain_to_table(lua, &chain)?)),
                    None => Ok(Value::Nil),
                }
            },
        );
        methods.add_method("lock_manifest", |_, this, vfs: AnyUserData| {
            let vfs = vfs.borrow::<LuaVfs>()?;
            this.0
                .lock_manifest(&vfs.0)
                .map(LuaVfsLock)
                .map_err(LuaError::external)
        });
        methods.add_method(
            "diff_against_lock",
            |lua, this, (vfs, lock): (AnyUserData, AnyUserData)| {
                let vfs = vfs.borrow::<LuaVfs>()?;
                let lock = lock.borrow::<LuaVfsLock>()?;
                drift_report_to_table(
                    lua,
                    &this
                        .0
                        .diff_against_lock(&vfs.0, &lock.0)
                        .map_err(LuaError::external)?,
                )
            },
        );
        methods.add_method(
            "semantic_conflicts",
            |lua, this, (vfs, opts): (AnyUserData, Option<Table>)| {
                let vfs = vfs.borrow::<LuaVfs>()?;
                let opts = semantic_opts_from_table(opts)?;
                semantic_conflict_report_to_table(
                    lua,
                    &this
                        .0
                        .semantic_conflicts_with_opts(&vfs.0, opts)
                        .map_err(LuaError::external)?,
                )
            },
        );
    }
}

impl UserData for LuaVfsLock {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("schema_version", |_, this, ()| Ok(this.0.schema_version));
        methods.add_method("entries", |lua, this, ()| {
            vfs_lock_entries_to_table(lua, &this.0.entries)
        });
        methods.add_method("to_table", |lua, this, ()| vfs_lock_to_table(lua, &this.0));
    }
}

impl UserData for LuaConflictIndex {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("sources", |lua, this, ()| {
            paths_to_table(lua, this.0.sources.iter())
        });
        methods.add_method("sources_containing", |lua, this, path: String| {
            indices_to_table(lua, this.0.sources_containing(Path::new(&path)))
        });
        methods.add_method("conflicts_report", |lua, this, relative: Option<bool>| {
            conflicts_report_to_table(lua, &this.0.conflicts_report(relative.unwrap_or(true)))
        });
        methods.add_method("shadowed_report", |lua, this, args: Variadic<Value>| {
            let relative = optional_bool_arg(args.first(), true)?;
            let list_files = optional_bool_arg(args.get(1), true)?;
            shadowed_report_to_table(
                lua,
                &this.0.shadowed_report_with_files(relative, list_files),
            )
        });
        methods.add_method("diff_report", |lua, this, (a, b): (String, String)| {
            diff_report_to_table(lua, &this.0.diff_report(Path::new(&a), Path::new(&b)))
        });
    }
}

impl UserData for LuaSnapshot {}

impl UserData for LuaMetadataSnapshot {}

fn optional_bool_arg(value: Option<&Value>, default: bool) -> LuaResult<bool> {
    match value {
        Some(Value::Boolean(value)) => Ok(*value),
        Some(Value::Nil) | None => Ok(default),
        Some(value) => Err(LuaError::external(format!(
            "expected boolean or nil, got {}",
            value.type_name()
        ))),
    }
}

fn lua_normalize_host_path(_: &Lua, path: String) -> String {
    let mut path = PathBuf::from(path);
    normalize_host_path_in_place(&mut path);
    path_to_string(path)
}

fn lua_analyze_pair(
    lua: &Lua,
    (path, left, right): (String, mlua::String, mlua::String),
) -> LuaResult<Table> {
    let (class, delta) = analyze_pair(
        Path::new(&path),
        left.as_bytes().as_ref(),
        right.as_bytes().as_ref(),
    );
    let table = lua.create_table()?;
    table.set("asset_class", asset_class_name(class))?;
    table.set("delta", semantic_delta_to_table(lua, &delta)?)?;
    Ok(table)
}

fn lua_run_setup(
    _: &Lua,
    (vfs, merged_dir, use_hardlinks): (AnyUserData, String, bool),
) -> LuaResult<(usize, LuaSnapshot)> {
    let vfs = vfs.borrow::<LuaVfs>()?;
    run_setup(&vfs.0, Path::new(&merged_dir), use_hardlinks)
        .map(|(count, snapshot)| (count, LuaSnapshot(snapshot)))
        .map_err(LuaError::external)
}

fn lua_run_setup_tracked(
    _: &Lua,
    (vfs, merged_dir, use_hardlinks): (AnyUserData, String, bool),
) -> LuaResult<(usize, LuaMetadataSnapshot)> {
    let vfs = vfs.borrow::<LuaVfs>()?;
    run_setup_tracked(&vfs.0, Path::new(&merged_dir), use_hardlinks)
        .map(|(count, snapshot)| (count, LuaMetadataSnapshot(snapshot)))
        .map_err(LuaError::external)
}

fn lua_run_finalize(
    lua: &Lua,
    (merged_dir, output_dir, snapshot): (String, String, AnyUserData),
) -> LuaResult<Table> {
    let snapshot = snapshot.borrow::<LuaSnapshot>()?;
    copied_to_table(
        lua,
        &run_finalize(Path::new(&merged_dir), &snapshot.0, Path::new(&output_dir))
            .map_err(LuaError::external)?,
    )
}

fn lua_run_finalize_tracked(
    lua: &Lua,
    (merged_dir, output_dir, snapshot): (String, String, AnyUserData),
) -> LuaResult<Table> {
    let snapshot = snapshot.borrow::<LuaMetadataSnapshot>()?;
    copied_to_table(
        lua,
        &run_finalize_tracked(Path::new(&merged_dir), &snapshot.0, Path::new(&output_dir))
            .map_err(LuaError::external)?,
    )
}

fn lua_changed_files(lua: &Lua, (dir, snapshot): (String, AnyUserData)) -> LuaResult<Table> {
    let snapshot = snapshot.borrow::<LuaSnapshot>()?;
    paths_to_table(
        lua,
        changed_files(Path::new(&dir), &snapshot.0)
            .map_err(LuaError::external)?
            .iter(),
    )
}

fn lua_changed_files_metadata(
    lua: &Lua,
    (dir, snapshot): (String, AnyUserData),
) -> LuaResult<Table> {
    let snapshot = snapshot.borrow::<LuaMetadataSnapshot>()?;
    paths_to_table(
        lua,
        changed_files_metadata(Path::new(&dir), &snapshot.0)
            .map_err(LuaError::external)?
            .iter(),
    )
}

#[cfg(feature = "serialize")]
fn lua_serialize(lua: &Lua, (value, format): (Value, String)) -> LuaResult<String> {
    use mlua::LuaSerdeExt;
    let value: serde_json::Value = lua.from_value(value)?;
    serialize_value(&value, serialize_type(&format)?).map_err(LuaError::external)
}

fn strings_from_sequence(table: &Table) -> LuaResult<Vec<String>> {
    table.sequence_values::<String>().collect()
}

fn pathbufs_from_sequence(table: &Table) -> LuaResult<Vec<PathBuf>> {
    strings_from_sequence(table).map(|items| items.into_iter().map(PathBuf::from).collect())
}

fn archive_list_from_opts(opts: Option<Table>) -> LuaResult<Option<Vec<String>>> {
    opts.map(|opts| match opts.get::<Option<Table>>("archives")? {
        Some(archives) => strings_from_sequence(&archives).map(Some),
        None => Ok(None),
    })
    .transpose()
    .map(Option::flatten)
}

fn semantic_opts_from_table(opts: Option<Table>) -> LuaResult<SemanticOpts> {
    let Some(opts) = opts else {
        return Ok(SemanticOpts::default());
    };
    let archive_hash_mode = match opts
        .get::<Option<String>>("archive_hash_mode")?
        .as_deref()
        .unwrap_or("winner_only")
    {
        "disabled" => ArchiveHashMode::Disabled,
        "winner_only" => ArchiveHashMode::WinnerOnly,
        "all_providers" => ArchiveHashMode::AllProviders,
        other => {
            return Err(LuaError::external(format!(
                "unknown archive hash mode: {other}"
            )));
        }
    };
    Ok(SemanticOpts {
        archive_hash_mode,
        include_semantic_deltas: opts.get("include_semantic_deltas").unwrap_or(false),
    })
}

fn collapse_options_from_table(opts: Option<Table>) -> CollapseOptions {
    let Some(opts) = opts else {
        return CollapseOptions {
            allow_copying: false,
            extract_archives: false,
            use_symlinks: false,
        };
    };
    CollapseOptions {
        allow_copying: opts.get("allow_copying").unwrap_or(false),
        extract_archives: opts.get("extract_archives").unwrap_or(false),
        use_symlinks: opts.get("use_symlinks").unwrap_or(false),
    }
}

fn source_meta_from_table(table: &Table) -> LuaResult<SourceMeta> {
    Ok(SourceMeta {
        path: PathBuf::from(table.get::<String>("path")?),
        kind: source_kind_from_name(&table.get::<String>("kind")?)?,
    })
}

fn source_kind_from_name(name: &str) -> LuaResult<SourceKind> {
    match name {
        "loose_dir" => Ok(SourceKind::LooseDir),
        "archive" => Ok(SourceKind::Archive),
        _ => Err(LuaError::external(format!("unknown source kind: {name}"))),
    }
}

fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::LooseDir => "loose_dir",
        SourceKind::Archive => "archive",
    }
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

fn path_to_string_ref(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn paths_to_table<'a>(lua: &Lua, paths: impl IntoIterator<Item = &'a PathBuf>) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, path) in paths.into_iter().enumerate() {
        table.set(index + 1, path_to_string(path.clone()))?;
    }
    Ok(table)
}

fn strings_to_table<'a>(
    lua: &Lua,
    values: impl IntoIterator<Item = &'a String>,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, value) in values.into_iter().enumerate() {
        table.set(index + 1, value.as_str())?;
    }
    Ok(table)
}

fn normalized_paths_to_table<'a>(
    lua: &Lua,
    paths: impl IntoIterator<Item = &'a NormalizedPath>,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, path) in paths.into_iter().enumerate() {
        table.set(index + 1, path_to_string(key_to_path_buf_lossy(path)))?;
    }
    Ok(table)
}

fn indices_to_table(lua: &Lua, indices: &[usize]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, source_index) in indices.iter().enumerate() {
        table.set(index + 1, *source_index)?;
    }
    Ok(table)
}

fn source_meta_to_table(lua: &Lua, source: &SourceMeta) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("path", path_to_string(source.path.clone()))?;
    table.set("kind", source_kind_name(source.kind))?;
    Ok(table)
}

fn sources_to_table(lua: &Lua, sources: &[SourceMeta]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, source) in sources.iter().enumerate() {
        table.set(index + 1, source_meta_to_table(lua, source)?)?;
    }
    Ok(table)
}

fn vfs_file_to_table(lua: &Lua, file: &VfsFile) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("path", path_to_string(file.path()))?;
    table.set("is_loose", file.is_loose())?;
    table.set("is_archive", file.is_archive())?;
    table.set("parent_archive_path", file.parent_archive_path())?;
    table.set("parent_archive_name", file.parent_archive_name())?;
    table.set("file", LuaVfsFile(file.clone()))?;
    Ok(table)
}

fn vfs_entries_to_table<'a>(
    lua: &Lua,
    entries: impl IntoIterator<Item = (&'a NormalizedPath, &'a VfsFile)>,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, (key, file)) in entries.into_iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(key_to_path_buf_lossy(key)))?;
        row.set("file", LuaVfsFile(file.clone()))?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn removed_vfs_files_to_table(
    lua: &Lua,
    entries: Vec<(NormalizedPath, VfsFile)>,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, (key, file)) in entries.into_iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(key_to_path_buf_lossy(&key)))?;
        row.set("file", LuaVfsFile(file))?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn display_tree_to_table(lua: &Lua, tree: &crate::DisplayTree) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (root, node) in tree {
        table.set(
            path_to_string(root.clone()),
            directory_node_to_table(lua, node)?,
        )?;
    }
    Ok(table)
}

fn directory_node_to_table(
    lua: &Lua,
    node: &crate::directory_node::DirectoryNode,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let files = lua.create_table()?;
    for (index, file) in node.files.iter().enumerate() {
        files.set(index + 1, vfs_file_to_table(lua, file)?)?;
    }
    let subdirs = lua.create_table()?;
    for (path, node) in &node.subdirs {
        subdirs.set(
            path_to_string(path.clone()),
            directory_node_to_table(lua, node)?,
        )?;
    }
    table.set("files", files)?;
    table.set("subdirs", subdirs)?;
    Ok(table)
}

fn provider_record_to_table(lua: &Lua, provider: &VfsProviderRecord) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("source_index", provider.source_index)?;
    table.set("source", source_meta_to_table(lua, &provider.source)?)?;
    table.set("key", path_to_string(provider.key.clone()))?;
    table.set(
        "original_path",
        path_to_string(provider.original_path.clone()),
    )?;
    table.set("resolved_path", provider.resolved_path.clone())?;
    Ok(table)
}

fn provider_records_to_table(lua: &Lua, providers: &[VfsProviderRecord]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, provider) in providers.iter().enumerate() {
        table.set(index + 1, provider_record_to_table(lua, provider)?)?;
    }
    Ok(table)
}

fn explain_to_table(lua: &Lua, report: &ExplainReport) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("key", path_to_string(report.key.clone()))?;
    table.set("winner", provider_record_to_table(lua, &report.winner)?)?;
    table.set(
        "overridden",
        provider_records_to_table(lua, &report.overridden)?,
    )?;
    Ok(table)
}

fn optional_explain_to_value(lua: &Lua, report: Option<ExplainReport>) -> LuaResult<Value> {
    match report {
        Some(report) => Ok(Value::Table(explain_to_table(lua, &report)?)),
        None => Ok(Value::Nil),
    }
}

fn duplicate_report_to_table(lua: &Lua, report: &DuplicateReport) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let entries = lua.create_table()?;
    for (index, entry) in report.entries.iter().enumerate() {
        entries.set(index + 1, duplicate_entry_to_table(lua, entry)?)?;
    }
    table.set("entries", entries)?;
    Ok(table)
}

fn duplicate_entry_to_table(lua: &Lua, entry: &DuplicateEntry) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("key", path_to_string(entry.key.clone()))?;
    table.set(
        "providers",
        provider_records_to_table(lua, &entry.providers)?,
    )?;
    table.set("winner_index", entry.winner_index)?;
    Ok(table)
}

fn archive_infos_to_table(lua: &Lua, infos: &[crate::ArchiveInfo]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, info) in infos.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("source_index", info.source_index)?;
        row.set("path", path_to_string(info.path.clone()))?;
        row.set("entry_count", info.entry_count)?;
        row.set("winning_entry_count", info.winning_entry_count)?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn archive_entries_to_table(lua: &Lua, entries: &[crate::ArchiveEntry]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, entry) in entries.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(entry.key.clone()))?;
        row.set("archive_path", path_to_string(entry.archive_path.clone()))?;
        row.set("original_path", path_to_string(entry.original_path.clone()))?;
        row.set("wins", entry.wins)?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn source_contribution_report_to_table(
    lua: &Lua,
    report: &SourceContributionReport,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let sources = lua.create_table()?;
    for (index, source) in report.sources.iter().enumerate() {
        sources.set(index + 1, source_contribution_to_table(lua, source)?)?;
    }
    table.set("sources", sources)?;
    Ok(table)
}

fn source_contribution_to_table(lua: &Lua, contribution: &SourceContribution) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("source_index", contribution.source_index)?;
    table.set("source", source_meta_to_table(lua, &contribution.source)?)?;
    table.set("winning_files", contribution.winning_files)?;
    table.set("overriding_files", contribution.overriding_files)?;
    table.set("overridden_files", contribution.overridden_files)?;
    table.set("unique_files", contribution.unique_files)?;
    table.set("duplicate_files", contribution.duplicate_files)?;
    table.set("loose_files", contribution.loose_files)?;
    table.set("archive_files", contribution.archive_files)?;
    Ok(table)
}

fn provider_record_to_lua_table(lua: &Lua, provider: &ProviderRecord) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("source", source_meta_to_table(lua, &provider.source)?)?;
    table.set("resolved_path", provider.resolved_path.clone())?;
    table.set("hash_blake3", provider.hash_blake3.clone())?;
    table.set("size", provider.size)?;
    Ok(table)
}

fn provenance_chain_to_table(lua: &Lua, chain: &ProvenanceChain) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("key", path_to_string(chain.key.clone()))?;
    table.set("winner", source_meta_to_table(lua, &chain.winner)?)?;
    let providers = lua.create_table()?;
    for (index, provider) in chain.providers.iter().enumerate() {
        providers.set(index + 1, provider_record_to_lua_table(lua, provider)?)?;
    }
    table.set("providers", providers)?;
    Ok(table)
}

fn vfs_lock_to_table(lua: &Lua, lock: &VfsLock) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("schema_version", lock.schema_version)?;
    table.set("entries", vfs_lock_entries_to_table(lua, &lock.entries)?)?;
    Ok(table)
}

fn vfs_lock_entries_to_table(lua: &Lua, entries: &[VfsLockEntry]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, entry) in entries.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(entry.key.clone()))?;
        row.set("winner_source", path_to_string(entry.winner_source.clone()))?;
        row.set("winner_kind", source_kind_name(entry.winner_kind))?;
        row.set("winner_hash_blake3", entry.winner_hash_blake3.clone())?;
        row.set("winner_size", entry.winner_size)?;
        row.set("provider_count", entry.provider_count)?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn drift_kind_name(kind: DriftKind) -> &'static str {
    match kind {
        DriftKind::Added => "added",
        DriftKind::Removed => "removed",
        DriftKind::WinnerSourceChanged => "winner_source_changed",
        DriftKind::WinnerHashChanged => "winner_hash_changed",
        DriftKind::ProviderCountChanged => "provider_count_changed",
    }
}

fn drift_report_to_table(lua: &Lua, report: &DriftReport) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let entries = lua.create_table()?;
    for (index, entry) in report.entries.iter().enumerate() {
        entries.set(index + 1, drift_entry_to_table(lua, entry)?)?;
    }
    let counts = lua.create_table()?;
    for (kind, count) in &report.counts {
        counts.set(drift_kind_name(*kind), *count)?;
    }
    table.set("entries", entries)?;
    table.set("counts", counts)?;
    Ok(table)
}

fn drift_entry_to_table(lua: &Lua, entry: &DriftEntry) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("key", path_to_string(entry.key.clone()))?;
    table.set("kind", drift_kind_name(entry.kind))?;
    Ok(table)
}

fn materialization_plan_to_table(lua: &Lua, plan: &MaterializationPlan) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let actions = lua.create_table()?;
    for (index, action) in plan.actions.iter().enumerate() {
        actions.set(index + 1, materialization_action_to_table(lua, action)?)?;
    }
    let issues = lua.create_table()?;
    for (index, issue) in plan.issues.iter().enumerate() {
        issues.set(index + 1, materialization_issue_to_table(lua, issue)?)?;
    }
    table.set("actions", actions)?;
    table.set("issues", issues)?;
    Ok(table)
}

fn materialization_action_to_table(lua: &Lua, action: &MaterializationAction) -> LuaResult<Table> {
    let table = lua.create_table()?;
    match action {
        MaterializationAction::Hardlink { key, source, dest } => {
            set_materialization_row(&table, "hardlink", key, Some(source), None, Some(dest))?;
        }
        MaterializationAction::Symlink { key, source, dest } => {
            set_materialization_row(&table, "symlink", key, Some(source), None, Some(dest))?;
        }
        MaterializationAction::Copy { key, source, dest } => {
            set_materialization_row(&table, "copy", key, Some(source), None, Some(dest))?;
        }
        MaterializationAction::ExtractArchive { key, archive, dest } => set_materialization_row(
            &table,
            "extract_archive",
            key,
            None,
            Some(archive),
            Some(dest),
        )?,
        MaterializationAction::SkipArchiveFile { key, archive } => {
            set_materialization_row(&table, "skip_archive_file", key, None, Some(archive), None)?;
        }
    }
    Ok(table)
}

fn set_materialization_row(
    table: &Table,
    kind: &str,
    key: &Path,
    source: Option<&PathBuf>,
    archive: Option<&PathBuf>,
    dest: Option<&PathBuf>,
) -> LuaResult<()> {
    table.set("kind", kind)?;
    table.set("key", path_to_string_ref(key))?;
    table.set("source", source.map(|path| path_to_string_ref(path)))?;
    table.set("archive", archive.map(|path| path_to_string_ref(path)))?;
    table.set("dest", dest.map(|path| path_to_string_ref(path)))?;
    Ok(())
}

fn materialization_issue_to_table(lua: &Lua, issue: &MaterializationIssue) -> LuaResult<Table> {
    let table = lua.create_table()?;
    match issue {
        MaterializationIssue::MissingLooseSource { key, source } => {
            table.set("kind", "missing_loose_source")?;
            table.set("key", path_to_string(key.clone()))?;
            table.set("source", path_to_string(source.clone()))?;
        }
        MaterializationIssue::FileDirectoryConflict { key, dest } => {
            table.set("kind", "file_directory_conflict")?;
            table.set("key", path_to_string(key.clone()))?;
            table.set("dest", path_to_string(dest.clone()))?;
        }
        MaterializationIssue::UnsafeDestination { key, dest } => {
            table.set("kind", "unsafe_destination")?;
            table.set("key", path_to_string(key.clone()))?;
            table.set("dest", path_to_string(dest.clone()))?;
        }
    }
    Ok(table)
}

fn directory_diff_to_table(lua: &Lua, diff: &DirectoryDiff<'_>) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let conflicts = lua.create_table()?;
    for (index, (key, incoming, current)) in diff.conflicts.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(key.clone()))?;
        row.set("incoming", LuaVfsFile(incoming.clone()))?;
        row.set("current", LuaVfsFile((*current).clone()))?;
        conflicts.set(index + 1, row)?;
    }
    let additions = lua.create_table()?;
    for (index, (key, file)) in diff.additions.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(key.clone()))?;
        row.set("file", LuaVfsFile(file.clone()))?;
        additions.set(index + 1, row)?;
    }
    table.set("conflicts", conflicts)?;
    table.set("additions", additions)?;
    Ok(table)
}

fn layer_providers_to_table(lua: &Lua, providers: &[LayerProvider]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, provider) in providers.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("source_index", provider.source_index)?;
        row.set("provider_index", provider.provider_index)?;
        row.set("source", source_meta_to_table(lua, &provider.source)?)?;
        row.set("key", path_to_string(provider.key.clone()))?;
        row.set(
            "original_path",
            path_to_string(provider.original_path.clone()),
        )?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn mutable_provider_to_table(lua: &Lua, provider: &VfsProvider) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("source", source_meta_to_table(lua, &provider.source)?)?;
    table.set("file", LuaVfsFile(provider.file.clone()))?;
    Ok(table)
}

fn mutable_providers_to_value(lua: &Lua, providers: &[VfsProvider]) -> LuaResult<Value> {
    Ok(Value::Table(mutable_providers_to_table(lua, providers)?))
}

fn mutable_providers_to_table(lua: &Lua, providers: &[VfsProvider]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, provider) in providers.iter().enumerate() {
        table.set(index + 1, mutable_provider_to_table(lua, provider)?)?;
    }
    Ok(table)
}

fn removed_providers_to_table(
    lua: &Lua,
    removed: &[(NormalizedPath, VfsProvider)],
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, (key, provider)) in removed.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(key_to_path_buf_lossy(key)))?;
        row.set("provider", mutable_provider_to_table(lua, provider)?)?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn conflicts_report_to_table(lua: &Lua, report: &ConflictsReport) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let sources = lua.create_table()?;
    for (index, source) in report.sources.iter().enumerate() {
        sources.set(index + 1, conflict_source_entry_to_table(lua, source)?)?;
    }
    table.set("sources", sources)?;
    Ok(table)
}

fn conflict_source_entry_to_table(lua: &Lua, source: &ConflictSourceEntry) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("path", path_to_string(source.path.clone()))?;
    table.set("overrides", paths_to_table(lua, source.overrides.iter())?)?;
    table.set(
        "overridden_by",
        paths_to_table(lua, source.overridden_by.iter())?,
    )?;
    Ok(table)
}

fn shadowed_report_to_table(lua: &Lua, report: &ShadowedReport) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let sources = lua.create_table()?;
    for (index, source) in report.sources.iter().enumerate() {
        sources.set(index + 1, shadowed_source_to_table(lua, source)?)?;
    }
    table.set("sources", sources)?;
    Ok(table)
}

fn shadowed_source_to_table(lua: &Lua, source: &ShadowedSource) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("path", path_to_string(source.path.clone()))?;
    table.set(
        "shadowed_files",
        paths_to_table(lua, source.shadowed_files.iter())?,
    )?;
    Ok(table)
}

fn diff_report_to_table(lua: &Lua, report: &DiffReport) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("source_a", path_to_string(report.source_a.clone()))?;
    table.set("source_b", path_to_string(report.source_b.clone()))?;
    table.set(
        "higher_priority",
        path_to_string(report.higher_priority.clone()),
    )?;
    table.set("shared", strings_to_table(lua, report.shared.iter())?)?;
    table.set("only_in_a", strings_to_table(lua, report.only_in_a.iter())?)?;
    table.set("only_in_b", strings_to_table(lua, report.only_in_b.iter())?)?;
    Ok(table)
}

fn copied_to_table(lua: &Lua, copied: &[(PathBuf, PathBuf)]) -> LuaResult<Table> {
    let table = lua.create_table()?;
    for (index, (relative_path, destination_path)) in copied.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("relative_path", path_to_string(relative_path.clone()))?;
        row.set("destination_path", path_to_string(destination_path.clone()))?;
        table.set(index + 1, row)?;
    }
    Ok(table)
}

fn asset_class_name(class: AssetClass) -> &'static str {
    match class {
        AssetClass::Ini => "ini",
        AssetClass::Toml => "toml",
        AssetClass::Json => "json",
        AssetClass::LuaScript => "lua_script",
        AssetClass::MwScriptLike => "mw_script_like",
        AssetClass::Text => "text",
        AssetClass::Binary => "binary",
        AssetClass::Unknown => "unknown",
    }
}

fn semantic_delta_to_table(lua: &Lua, delta: &SemanticDelta) -> LuaResult<Table> {
    let table = lua.create_table()?;
    match delta {
        SemanticDelta::NoOpEquivalent => table.set("kind", "no_op_equivalent")?,
        SemanticDelta::CosmeticOnly => table.set("kind", "cosmetic_only")?,
        SemanticDelta::BehaviorChanging { change_summary } => {
            table.set("kind", "behavior_changing")?;
            let changes = lua.create_table()?;
            for (index, change) in change_summary.iter().enumerate() {
                changes.set(index + 1, change.clone())?;
            }
            table.set("change_summary", changes)?;
        }
        SemanticDelta::Unknown => table.set("kind", "unknown")?,
    }
    Ok(table)
}

fn semantic_relation_name(relation: SemanticRelation) -> &'static str {
    match relation {
        SemanticRelation::IdenticalToWinner => "identical_to_winner",
        SemanticRelation::DifferentFromWinner => "different_from_winner",
        SemanticRelation::Unknown => "unknown",
    }
}

fn semantic_provider_to_table(lua: &Lua, provider: &SemanticProvider) -> LuaResult<Table> {
    let table = lua.create_table()?;
    table.set("source", source_meta_to_table(lua, &provider.source)?)?;
    table.set("relation", semantic_relation_name(provider.relation))?;
    table.set("hash_blake3", provider.hash_blake3.clone())?;
    table.set("size", provider.size)?;
    match &provider.semantic_delta_to_winner {
        Some(delta) => table.set(
            "semantic_delta_to_winner",
            semantic_delta_to_table(lua, delta)?,
        )?,
        None => table.set("semantic_delta_to_winner", Value::Nil)?,
    }
    Ok(table)
}

fn semantic_conflict_report_to_table(
    lua: &Lua,
    report: &SemanticConflictReport,
) -> LuaResult<Table> {
    let table = lua.create_table()?;
    let entries = lua.create_table()?;
    for (index, entry) in report.entries.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("key", path_to_string(entry.key.clone()))?;
        row.set("winner", source_meta_to_table(lua, &entry.winner)?)?;
        let providers = lua.create_table()?;
        for (provider_index, provider) in entry.providers.iter().enumerate() {
            providers.set(
                provider_index + 1,
                semantic_provider_to_table(lua, provider)?,
            )?;
        }
        row.set("providers", providers)?;
        row.set("asset_class", asset_class_name(entry.asset_class))?;
        row.set("all_identical", entry.all_identical)?;
        row.set("distinct_versions", entry.distinct_versions)?;
        entries.set(index + 1, row)?;
    }
    table.set("entries", entries)?;
    Ok(table)
}

#[cfg(feature = "serialize")]
fn serialize_type(format: &str) -> LuaResult<SerializeType> {
    match format {
        "json" => Ok(SerializeType::Json),
        "yaml" => Ok(SerializeType::Yaml),
        "toml" => Ok(SerializeType::Toml),
        _ => Err(LuaError::external(format!(
            "unknown serialization format: {format}"
        ))),
    }
}
