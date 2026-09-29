// SPDX-License-Identifier: MIT OR Apache-2.0
//! `dream.vfs.ConflictIndex`: the derived source-versus-source conflict projection.

use l3i::{
    Error, Result,
    bind::Call,
    extension::{ExtensionDescriptor, TagPolicy},
    options::Options,
    stack::{Scope, ValueView},
    userdata::{Owned, Userdata},
    value::Table,
};

use super::{
    ConflictIndexHandle, LayerIndexHandle, class_function, frozen_class_table, host_path,
    paths_from_array, paths_from_table, reports, types,
};
use crate::ConflictIndex;

/// The `ConflictIndex` class table's type.
pub(super) const CLASS_TYPE: &str = "{ fromDirectories: (dirs: { string }) -> dream_vfs_ConflictIndex, \
    fromFileLists: (sources: { { source: string, files: { string } } }) -> dream_vfs_ConflictIndex, \
    fromLayerIndex: (layer: dream_vfs_LayerIndex) -> dream_vfs_ConflictIndex }";

pub(super) fn class_table(runtime: &l3i::Runtime) -> Result<Table> {
    frozen_class_table(runtime, |table| {
        class_function(
            runtime,
            table,
            "ConflictIndex",
            "fromDirectories",
            |call: &Call, dirs: ValueView| {
                let dirs = paths_from_array(call, dirs, "dirs")?;
                Ok::<_, Error>(Owned(ConflictIndexHandle(ConflictIndex::from_directories(
                    dirs.iter(),
                ))))
            },
        )?;
        class_function(
            runtime,
            table,
            "ConflictIndex",
            "fromFileLists",
            |call: &Call, sources: ValueView| {
                let sources_table = sources.as_table()?;
                let mut rows = Vec::with_capacity(sources_table.raw_len());
                call.with_frame(|frame| {
                    sources_table.for_each_array(frame, |frame, index, row| {
                        let context = format!("ConflictIndex.fromFileLists[{index}]");
                        let row = Options::read(frame, row, &context, |o| {
                            let source =
                                o.required_bytes("source", |source| Ok(host_path(source)))?;
                            let files_path = format!("{}.files", o.context());
                            let files = o.required_table("files", |frame, files| {
                                paths_from_table(frame, &files, &files_path)
                            })?;
                            Ok((source, files))
                        })?;
                        rows.push(row);
                        Ok(())
                    })
                })?;
                Ok::<_, Error>(Owned(ConflictIndexHandle(ConflictIndex::from_file_lists(
                    rows,
                ))))
            },
        )?;
        class_function(
            runtime,
            table,
            "ConflictIndex",
            "fromLayerIndex",
            |layer: &LayerIndexHandle| {
                Owned(ConflictIndexHandle(ConflictIndex::from_layer_index(
                    &layer.0,
                )))
            },
        )
    })
}

pub(super) fn describe(d: &mut ExtensionDescriptor) {
    let mut conflicts = d.userdata::<ConflictIndexHandle>(ConflictIndexHandle::NAME);
    conflicts
        .tag(TagPolicy::Never)
        .doc("Per-source override and overridden-by sets over a load order.");
    conflicts
        .method("sources", |c: &ConflictIndexHandle, call: &Call| {
            reports::path_list(call, c.0.sources.iter())
        })
        .signature("(self): { string }");
    conflicts.method("sourcesContaining", |c: &ConflictIndexHandle, call: &Call, path: &[u8]| {
        reports::indices(call, c.0.sources_containing(&host_path(path)))
    })
    .signature("(self, path: string): { number }")
    .doc("The 1-based source positions that provide a conflicting key; empty when only one source has it.");
    conflicts
        .method(
            "conflictsReport",
            |c: &ConflictIndexHandle, call: &Call, relative: Option<bool>| {
                reports::conflicts_report(call, &c.0.conflicts_report(relative.unwrap_or(true)))
            },
        )
        .signature(format!(
            "(self, relative: boolean?): {}",
            types::CONFLICTS_REPORT
        ));
    conflicts
        .method(
            "shadowedReport",
            |c: &ConflictIndexHandle,
             call: &Call,
             relative: Option<bool>,
             list_files: Option<bool>| {
                reports::shadowed_report(
                    call,
                    &c.0.shadowed_report_with_files(
                        relative.unwrap_or(true),
                        list_files.unwrap_or(true),
                    ),
                )
            },
        )
        .signature(format!(
            "(self, relative: boolean?, listFiles: boolean?): {}",
            types::SHADOWED_REPORT
        ));
    conflicts
        .method(
            "diffReport",
            |c: &ConflictIndexHandle, call: &Call, a: &[u8], b: &[u8]| {
                reports::diff_report(call, &c.0.diff_report(&host_path(a), &host_path(b)))
            },
        )
        .signature(format!(
            "(self, sourceA: string, sourceB: string): {}",
            types::DIFF_REPORT
        ));
    conflicts.metamethod("__tostring", |c: &ConflictIndexHandle| {
        format!("dream.vfs.ConflictIndex({} sources)", c.0.sources.len())
    });
}
