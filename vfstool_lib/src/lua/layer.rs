// SPDX-License-Identifier: MIT OR Apache-2.0
//! `dream.vfs.LayerIndex`: the canonical provider-occurrence index and its analyses.

use l3i::{
    Error, Result,
    bind::{Call, StackResults},
    convert::Exact,
    extension::{ExtensionDescriptor, TagPolicy, UserdataBuilder},
    options::Options,
    stack::{Scope, ValueView},
    userdata::{Owned, Userdata},
    value::Table,
};

use super::{
    LayerIndexHandle, Vfs, VfsLockHandle, class_function, frozen_class_table,
    handles::source_meta,
    host_path, index_from_lua, index_to_lua, io_error, paths_from_table, reports, types,
    views::{self, KeyBlob},
};
use crate::{ArchiveHashMode, LayerIndex, SemanticOpts, SourceId};

fn semantic_options(scope: &impl Scope, options: Option<ValueView<'_>>) -> Result<SemanticOpts> {
    let mut opts = SemanticOpts::default();
    let Some(options) = options.filter(|view| !view.is_nil()) else {
        return Ok(opts);
    };
    Options::read(scope, options, "layer:semanticConflicts", |o| {
        if let Some(mode) = o.optional_str("archiveHashMode", |mode| match mode {
            "disabled" => Ok(ArchiveHashMode::Disabled),
            "winnerOnly" => Ok(ArchiveHashMode::WinnerOnly),
            "allProviders" => Ok(ArchiveHashMode::AllProviders),
            other => Err(Error::runtime(format!(
                "expected 'disabled', 'winnerOnly', or 'allProviders', got '{other}'"
            ))),
        })? {
            opts.archive_hash_mode = mode;
        }
        opts.include_semantic_deltas = o.or("includeSemanticDeltas", false)?;
        Ok(())
    })?;
    Ok(opts)
}

pub(super) fn class_table(runtime: &l3i::Runtime) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(
            runtime,
            table,
            "LayerIndex",
            "fromFileLists",
            |call: &Call, sources: ValueView| {
                let sources_table = sources.as_table()?;
                let mut rows = Vec::with_capacity(sources_table.raw_len());
                call.with_frame(|frame| {
                    sources_table.for_each_array(frame, |frame, index, row| {
                        let context = format!("LayerIndex.fromFileLists[{index}]");
                        let row = Options::read(frame, row, &context, |o| {
                            let source = o.required_table("source", |frame, source| {
                                source_meta(frame, source.value(), "source")
                            })?;
                            let files = o.required_table("files", |frame, files| {
                                paths_from_table(frame, &files, "files")
                            })?;
                            Ok((source, files))
                        })?;
                        rows.push(row);
                        Ok(())
                    })
                })?;
                Ok::<_, Error>(Owned(LayerIndexHandle(LayerIndex::from_file_lists(rows))))
            },
        )
    })
}

pub(super) fn describe(d: &mut ExtensionDescriptor) {
    let mut layer = d.userdata::<LayerIndexHandle>(LayerIndexHandle::NAME);
    layer
        .tag(TagPolicy::Never)
        .doc("The provider-occurrence index: every provider of every key, low to high priority.");
    layer
        .method("keys", |l: &LayerIndexHandle, call: &Call| {
            views::push_keys(call, KeyBlob::sorted(l.0.keys())).map(drop)?;
            Ok::<_, Error>(StackResults)
        })
        .signature("(self): dream_vfs_Keys");
    layer
        .method("sources", |l: &LayerIndexHandle, call: &Call| {
            reports::sources(call, &l.0.sources)
        })
        .signature(format!("(self): {{ {} }}", types::SOURCE));
    layer
        .method("sourceIdForPath", |l: &LayerIndexHandle, path: &[u8]| {
            l.0.source_id_for_path(&host_path(path))
                .map(|id| index_to_lua(id.as_index()))
        })
        .signature("(self, path: string): number?")
        .doc("The 1-based load-order position of a source path, or nil.");
    layer
        .method(
            "sourceById",
            |l: &LayerIndexHandle, call: &Call, id: Exact<i64>| {
                let index = index_from_lua("source id", id)?;
                match l.0.source_by_id(SourceId::from_index(index)) {
                    Some(source) => reports::source_meta(call, source).map(Some),
                    None => Ok(None),
                }
            },
        )
        .signature(format!("(self, id: number): {}?", types::SOURCE));
    layer
        .method(
            "sourcesContaining",
            |l: &LayerIndexHandle, call: &Call, path: &[u8]| {
                reports::indices(call, l.0.sources_containing(path))
            },
        )
        .signature("(self, path: string): { number }")
        .doc("The 1-based source positions providing a key, low to high priority.");
    layer
        .method(
            "providerOriginalPath",
            |l: &LayerIndexHandle, source_index: Exact<i64>, path: &[u8]| {
                let index = index_from_lua("source index", source_index)?;
                Ok::<_, Error>(
                    l.0.provider_original_path(index, path)
                        .map(super::handles::path_string),
                )
            },
        )
        .signature("(self, sourceIndex: number, path: string): string?");
    layer
        .method(
            "providerChain",
            |l: &LayerIndexHandle, call: &Call, path: &[u8]| {
                reports::layer_providers(call, &l.0.provider_chain(&host_path(path)))
            },
        )
        .signature(format!(
            "(self, path: string): {{ {} }}",
            types::LAYER_PROVIDER
        ));
    layer
        .method("duplicateKeys", |l: &LayerIndexHandle, call: &Call| {
            views::push_keys(call, KeyBlob::sorted(l.0.duplicate_keys())).map(drop)?;
            Ok::<_, Error>(StackResults)
        })
        .signature("(self): dream_vfs_Keys");
    layer
        .method(
            "sourceContributions",
            |l: &LayerIndexHandle, call: &Call| {
                reports::source_contributions(call, &l.0.source_contributions())
            },
        )
        .signature(format!("(self): {}", types::CONTRIBUTIONS));
    describe_analyses(&mut layer);
}

fn describe_analyses(layer: &mut UserdataBuilder<'_, LayerIndexHandle>) {
    layer
        .method(
            "provenance",
            |l: &LayerIndexHandle, call: &Call, vfs: &Vfs, path: &[u8], with_hashes: bool| match vfs
                .with(|vfs| l.0.provenance(vfs, &host_path(path), with_hashes))?
                .map_err(io_error)?
            {
                Some(chain) => reports::provenance(call, &chain).map(Some),
                None => Ok(None),
            },
        )
        .signature(format!(
            "(self, vfs: dream_vfs_VFS, path: string, withHashes: boolean): {}?",
            types::PROVENANCE
        ));
    layer
        .method("lockManifest", |l: &LayerIndexHandle, vfs: &Vfs| {
            vfs.with(|vfs| l.0.lock_manifest(vfs))?
                .map(|lock| Owned(VfsLockHandle(lock)))
                .map_err(io_error)
        })
        .signature("(self, vfs: dream_vfs_VFS): dream_vfs_VfsLock");
    layer
        .method(
            "diffAgainstLock",
            |l: &LayerIndexHandle, call: &Call, vfs: &Vfs, lock: &VfsLockHandle| {
                let report = vfs
                    .with(|vfs| l.0.diff_against_lock(vfs, &lock.0))?
                    .map_err(io_error)?;
                reports::drift(call, &report)
            },
        )
        .signature(format!(
            "(self, vfs: dream_vfs_VFS, lock: dream_vfs_VfsLock): {}",
            types::DRIFT
        ));
    layer.method("semanticConflicts", |l: &LayerIndexHandle, call: &Call, vfs: &Vfs, options: Option<ValueView>| {
        let opts = semantic_options(call, options)?;
        let report = vfs.with(|vfs| l.0.semantic_conflicts_with_opts(vfs, opts))?.map_err(io_error)?;
        reports::semantic_conflicts(call, &report)
    })
    .signature(format!(
        "(self, vfs: dream_vfs_VFS, options: {{ archiveHashMode: string?, includeSemanticDeltas: boolean? }}?): {}",
        types::SEMANTIC
    ));
    layer.metamethod("__tostring", |l: &LayerIndexHandle| {
        format!("dream.vfs.LayerIndex({} sources)", l.0.sources.len())
    });
}
