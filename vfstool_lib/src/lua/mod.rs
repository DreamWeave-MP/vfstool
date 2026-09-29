// SPDX-License-Identifier: MIT OR Apache-2.0
//! The `@dream/vfs` Luau module: the promoted stable `vfstool_lib` API as an l3i extension.
//!
//! This module is available with the `lua` feature. It never creates a VM and never installs a
//! global: the host composes [`VfsExtension`](crate::lua::VfsExtension) (together with dream-path's `PathExtension`, which
//! it requires) into an [`l3i::extension::RuntimePlan`], and every runtime made from that plan
//! can `require("@dream/vfs")`. A host that still wants the historical `vfstool` global exposes
//! the module as a compatibility global (`RuntimePolicy::compat_global(MODULE, MODULE_NAME)`).
//!
//! The module binds only the stable top-level API surface and methods on those stable types. It
//! does not bind `experimental`, low-level archive internals, or implementation details that
//! merely happen to be public Rust modules.
//!
//! Shapes, in the order a script meets them:
//!
//! - Paths are Luau strings read as bytes: no UTF-8 requirement anywhere, and a VFS key is
//!   normalized once, into a thread-local scratch buffer, before a lookup that allocates nothing.
//! - `VFS` and `VfsFile` are tagged userdata; `VfsProvider`, `LayerIndex`, `ConflictIndex`,
//!   `VfsLock`, and the snapshots are untagged userdata.
//! - Lists of keys and entries (`keys`, `entries`, `pathsMatching`, `pathsWith`, `providersFor`,
//!   `providerRecordsFor`, `layer:keys`, `layer:duplicateKeys`, `filesFromArchive`) are
//!   sequence views: `#items`, `items[i]` (1-based), `for _, item in items`, and
//!   `items:toTable()` for the plain table the previous binding returned. `tree`, `findByRegex`,
//!   and `remaining` are a stream of `{ dir, file }` rows with `toTable()` for the nested shape.
//! - Reports are plain tables with camelCase fields. Indices in them are 1-based. Byte sizes are
//!   Luau integers.
//! - Option tables are strict: an unknown key is an error, never a silently ignored default.
//!
//! ```no_run
//! use l3i::Runtime;
//! use l3i::extension::{RuntimePlan, RuntimePolicy};
//! use vfstool_lib::lua::{MODULE, MODULE_NAME, VfsExtension};
//!
//! let policy = RuntimePolicy::new().compat_global(MODULE, MODULE_NAME);
//! let plan = RuntimePlan::builder()
//!     .policy(policy)
//!     .extension(dream_path::lua::PathExtension)
//!     .extension(VfsExtension)
//!     .finalize()?;
//! let runtime = Runtime::from_plan(&plan)?;
//! runtime.exec(r#"local vfs = require("@dream/vfs").VFS.new() assert(vfs:len() == 0)"#)?;
//! # Ok::<(), l3i::Error>(())
//! ```

// Counts, lengths, and indices here come from Luau tables and Rust collections that are far
// below 2^52 entries, and pushed as the numbers scripts see; the cast lints have nothing to add.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

mod conflicts;
mod handles;
mod layer;
mod reports;
#[cfg(feature = "serialize")]
mod serialize;
mod vfs;
mod views;

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use dream_path::{is_normalized_path, normalize_path_into};
use l3i::{
    Error, Result,
    bind::Call,
    convert::Exact,
    extension::{Extension, ExtensionDescriptor, InstallContext},
    stack::{Frame, Scope, TableView, Type, ValueView},
    userdata::Owned,
    value::{Function, Table},
};

pub use handles::{
    ConflictIndexHandle, LayerIndexHandle, MetadataSnapshotHandle, SnapshotHandle, Vfs,
    VfsFileHandle, VfsLockHandle, VfsProviderHandle,
};
pub use views::{Entries, Keys, ProviderRecords, Providers, TreeWalk};

use crate::{
    analyze_pair, changed_files, changed_files_metadata, normalize_host_path_in_place,
    path_glob_matches, run_finalize, run_finalize_tracked, run_setup, run_setup_tracked,
    snapshot_directory, snapshot_directory_metadata, source_glob_matches,
};

/// The extension id.
pub const EXTENSION_ID: &str = "dream.vfs";

/// The module's `require` path.
pub const MODULE: &str = "@dream/vfs";

/// The conventional compatibility global name, for hosts that expose the module as a global
/// (`RuntimePolicy::compat_global(MODULE, MODULE_NAME)`). The module itself never installs it.
pub const MODULE_NAME: &str = "vfstool";

/// Luau type spellings shared by the declared signatures.
mod types {
    pub const SOURCE: &str = "{ path: string, kind: string }";
    /// One row of an `Entries` view.
    pub const ENTRY: &str = "{ key: string, file: dream_vfs_VfsFile }";
    /// One row of a `Providers` view, and of the provider lists mutation returns.
    pub const PROVIDER: &str =
        "{ source: { path: string, kind: string }, file: dream_vfs_VfsFile }";
    /// One row of a `ProviderRecords` view, and the rows of explain and duplicates reports.
    pub const PROVIDER_RECORD: &str = "{ sourceIndex: number, source: { path: string, kind: string }, key: string, originalPath: string, resolvedPath: string }";
    /// One row of the `Tree` stream.
    pub const TREE_ROW: &str = "{ dir: string, file: dream_vfs_VfsFile }";
    pub const LAYER_PROVIDER: &str = "{ sourceIndex: number, providerIndex: number, source: { path: string, kind: string }, key: string, originalPath: string }";
    pub const EXPLAIN: &str = "{ key: string, winner: { sourceIndex: number, source: { path: string, kind: string }, key: string, originalPath: string, resolvedPath: string }, overridden: { { sourceIndex: number, source: { path: string, kind: string }, key: string, originalPath: string, resolvedPath: string } } }";
    pub const DUPLICATES: &str = "{ entries: { { key: string, providers: { { sourceIndex: number, source: { path: string, kind: string }, key: string, originalPath: string, resolvedPath: string } }, winnerIndex: number } } }";
    pub const ARCHIVE_INFO: &str =
        "{ sourceIndex: number, path: string, entryCount: number, winningEntryCount: number }";
    pub const ARCHIVE_ENTRY: &str =
        "{ key: string, archivePath: string, originalPath: string, wins: boolean }";
    pub const CONTRIBUTIONS: &str = "{ sources: { { [string]: any } } }";
    pub const PLAN: &str =
        "{ actions: { { [string]: string? } }, issues: { { [string]: string? } } }";
    pub const DIRECTORY_DIFF: &str = "{ conflicts: { { key: string, incoming: dream_vfs_VfsFile, current: dream_vfs_VfsFile } }, additions: { { key: string, file: dream_vfs_VfsFile } } }";
    pub const REMOVED_PROVIDERS: &str = "{ { key: string, provider: { source: { path: string, kind: string }, file: dream_vfs_VfsFile } } }";
    pub const REMOVED_FILES: &str = "{ { key: string, file: dream_vfs_VfsFile } }";
    pub const PROVENANCE: &str = "{ key: string, winner: { path: string, kind: string }, providers: { { source: { path: string, kind: string }, resolvedPath: string, hashBlake3: string?, size: integer? } } }";
    pub const DRIFT: &str =
        "{ entries: { { key: string, kind: string } }, counts: { [string]: number } }";
    pub const SEMANTIC: &str = "{ entries: { { [string]: any } } }";
    pub const LOCK_ENTRIES: &str = "{ { key: string, winnerSource: string, winnerKind: string, winnerHashBlake3: string?, winnerSize: integer?, providerCount: number } }";
    pub const CONFLICTS_REPORT: &str =
        "{ sources: { { path: string, overrides: { string }, overriddenBy: { string } } } }";
    pub const SHADOWED_REPORT: &str =
        "{ sources: { { path: string, shadowedFiles: { string } } } }";
    pub const DIFF_REPORT: &str = "{ sourceA: string, sourceB: string, higherPriority: string, shared: { string }, onlyInA: { string }, onlyInB: { string } }";
    pub const SEMANTIC_PAIR: &str =
        "{ assetClass: string, delta: { kind: string, changeSummary: { string }? } }";
    pub const COPIED: &str = "{ { relativePath: string, destinationPath: string } }";
    pub const COLLAPSE_OPTIONS: &str =
        "{ allowCopying: boolean?, extractArchives: boolean?, useSymlinks: boolean? }?";
}

/// The `dream.vfs` extension: provides `@dream/vfs` and requires `dream.path`.
#[derive(Clone, Copy, Debug, Default)]
pub struct VfsExtension;

impl Extension for VfsExtension {
    fn id(&self) -> &'static str {
        EXTENSION_ID
    }

    fn describe(&self, d: &mut ExtensionDescriptor) -> Result<()> {
        d.requires(dream_path::lua::EXTENSION_ID);
        views::describe(d);
        vfs::describe(d);
        handles::describe_file(d);
        handles::describe_provider(d);
        layer::describe(d);
        conflicts::describe(d);
        handles::describe_snapshots(d);
        describe_module(d);
        Ok(())
    }

    /// The class tables (`VFS.new`, `VfsFile.from`, ...) are nested tables of bound functions,
    /// which only a live VM can hold, so they are filled here.
    fn install(&self, cx: &mut InstallContext<'_>) -> Result<()> {
        let runtime = cx.runtime();
        let module = cx.module(MODULE)?;
        module.set("VFS", &vfs::class_table(runtime)?)?;
        module.set("VfsFile", &handles::file_class_table(runtime)?)?;
        module.set("VfsProvider", &handles::provider_class_table(runtime)?)?;
        module.set("LayerIndex", &layer::class_table(runtime)?)?;
        module.set("ConflictIndex", &conflicts::class_table(runtime)?)?;
        Ok(())
    }
}

/// The module's free functions and its class tables.
fn describe_module(d: &mut ExtensionDescriptor) {
    let module = d.module(MODULE);
    module.doc("vfstool: OpenMW-style virtual file systems, provider analysis, and run workflows.");
    module
        .function("normalizeHostPath", normalize_host_path)
        .signature("(path: string) -> string")
        .doc("Backslashes to slashes and ASCII lowercase, for textual host path comparisons.")
        .function("normalizeHostPathInPlace", normalize_host_path)
        .signature("(path: string) -> string")
        .doc("The same as normalizeHostPath; Luau strings are immutable, so the name is kept for compatibility only.")
        .function("pathGlobMatches", |glob: &str, path: &[u8]| path_glob_matches(glob, &host_path(path)))
        .signature("(glob: string, path: string) -> boolean")
        .function("sourceGlobMatches", |glob: &str, source: &[u8]| source_glob_matches(glob, &host_path(source)))
        .signature("(glob: string, sourcePath: string) -> boolean")
        .function("analyzePair", |call: &Call, path: &[u8], left: &[u8], right: &[u8]| {
            let (class, delta) = analyze_pair(&host_path(path), left, right);
            reports::semantic_pair(call, class, &delta)
        })
        .signature(format!("(path: string, leftBytes: string, rightBytes: string) -> {}", types::SEMANTIC_PAIR))
        .function("runSetup", |call: &Call, vfs: &Vfs, merged_dir: &[u8], use_hardlinks: bool| {
            let (count, snapshot) = vfs.with(|vfs| run_setup(vfs, &host_path(merged_dir), use_hardlinks))?.map_err(io_error)?;
            call.push(&(count as f64))?;
            l3i::userdata::push_owned(call, SnapshotHandle(snapshot))?;
            Ok::<_, Error>(l3i::bind::StackResults)
        })
        .signature("(vfs: dream_vfs_VFS, mergedDir: string, useHardlinks: boolean) -> (number, dream_vfs_Snapshot)")
        .doc("Dumps the VFS into mergedDir and snapshots it. Hardlinks let child tools edit the originals in place.")
        .function("runSetupTracked", |call: &Call, vfs: &Vfs, merged_dir: &[u8], use_hardlinks: bool| {
            let (count, snapshot) = vfs.with(|vfs| run_setup_tracked(vfs, &host_path(merged_dir), use_hardlinks))?.map_err(io_error)?;
            call.push(&(count as f64))?;
            l3i::userdata::push_owned(call, MetadataSnapshotHandle(snapshot))?;
            Ok::<_, Error>(l3i::bind::StackResults)
        })
        .signature("(vfs: dream_vfs_VFS, mergedDir: string, useHardlinks: boolean) -> (number, dream_vfs_MetadataSnapshot)")
        .function("runFinalize", |call: &Call, merged_dir: &[u8], output_dir: &[u8], snapshot: &SnapshotHandle| {
            let copied = run_finalize(&host_path(merged_dir), &snapshot.0, &host_path(output_dir)).map_err(io_error)?;
            reports::copied(call, &copied)
        })
        .signature(format!("(mergedDir: string, outputDir: string, snapshot: dream_vfs_Snapshot) -> {}", types::COPIED))
        .function("runFinalizeTracked", |call: &Call, merged_dir: &[u8], output_dir: &[u8], snapshot: &MetadataSnapshotHandle| {
            let copied = run_finalize_tracked(&host_path(merged_dir), &snapshot.0, &host_path(output_dir)).map_err(io_error)?;
            reports::copied(call, &copied)
        })
        .signature(format!("(mergedDir: string, outputDir: string, snapshot: dream_vfs_MetadataSnapshot) -> {}", types::COPIED))
        .function("snapshotDirectory", |dir: &[u8]| {
            snapshot_directory(&host_path(dir)).map(|snapshot| Owned(SnapshotHandle(snapshot))).map_err(io_error)
        })
        .signature("(dir: string) -> dream_vfs_Snapshot")
        .function("snapshotDirectoryMetadata", |dir: &[u8]| {
            snapshot_directory_metadata(&host_path(dir))
                .map(|snapshot| Owned(MetadataSnapshotHandle(snapshot)))
                .map_err(io_error)
        })
        .signature("(dir: string) -> dream_vfs_MetadataSnapshot")
        .function("changedFiles", |call: &Call, dir: &[u8], snapshot: &SnapshotHandle| {
            let changed = changed_files(&host_path(dir), &snapshot.0).map_err(io_error)?;
            reports::path_list(call, changed.iter())
        })
        .signature("(dir: string, snapshot: dream_vfs_Snapshot) -> { string }")
        .function("changedFilesMetadata", |call: &Call, dir: &[u8], snapshot: &MetadataSnapshotHandle| {
            let changed = changed_files_metadata(&host_path(dir), &snapshot.0).map_err(io_error)?;
            reports::path_list(call, changed.iter())
        })
        .signature("(dir: string, snapshot: dream_vfs_MetadataSnapshot) -> { string }");
    #[cfg(feature = "serialize")]
    module
        .function("serialize", serialize::serialize)
        .signature("(value: any, format: string) -> string")
        .doc("Encodes a value as json, yaml, or toml; tables, sequence views, strings, numbers, booleans.");
    module
        .installed("VFS")
        .signature(vfs::CLASS_TYPE)
        .doc("VFS constructors.")
        .installed("VfsFile")
        .signature("{ from: (path: string) -> dream_vfs_VfsFile }")
        .installed("VfsProvider")
        .signature("{ new: (source: { path: string, kind: string }, file: dream_vfs_VfsFile) -> dream_vfs_VfsProvider }")
        .installed("LayerIndex")
        .signature("{ fromFileLists: (sources: { { source: { path: string, kind: string }, files: { string } } }) -> dream_vfs_LayerIndex }")
        .installed("ConflictIndex")
        .signature(conflicts::CLASS_TYPE);
}

fn normalize_host_path(bytes: &[u8]) -> Vec<u8> {
    let mut path = host_path(bytes);
    normalize_host_path_in_place(&mut path);
    path.into_os_string().into_encoded_bytes()
}

// ---------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------

thread_local! {
    /// The per-thread key normalization scratch, reused by every lookup on the thread.
    static SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Runs `body` on the normalized spelling of `key` without allocating: an already-normalized
/// key is passed through, anything else goes through the thread's scratch buffer.
pub(crate) fn with_key<R>(key: &[u8], body: impl FnOnce(&[u8]) -> R) -> R {
    if is_normalized_path(key) {
        return body(key);
    }
    SCRATCH.with(|scratch| match scratch.try_borrow_mut() {
        Ok(mut scratch) => {
            normalize_path_into(&mut scratch, key);
            body(&scratch)
        }
        // A nested borrow would mean a re-entrant call, which is served from a fresh buffer.
        Err(_) => body(&dream_path::normalize_path(key)),
    })
}

/// A host path from Luau string bytes: exact on Unix, lossy where the OS insists on Unicode.
pub(crate) fn host_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The bytes a host path is pushed as.
pub(crate) fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// `map_err` adapters take the error by value; these consume it into the message.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn io_error(error: std::io::Error) -> Error {
    Error::runtime(format!("dream.vfs: {error}"))
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn regex_error(error: regex::Error) -> Error {
    Error::runtime(format!("dream.vfs: invalid regex: {error}"))
}

/// A 1-based script index as the 0-based Rust index it names.
pub(crate) fn index_from_lua(what: &str, index: Exact<i64>) -> Result<usize> {
    usize::try_from(index.0)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .ok_or_else(|| {
            Error::runtime(format!(
                "dream.vfs: {what} must be a 1-based index, got {}",
                index.0
            ))
        })
}

/// A 0-based Rust index as the 1-based number scripts see.
pub(crate) fn index_to_lua(index: usize) -> f64 {
    (index + 1) as f64
}

pub(crate) fn source_kind_name(kind: crate::SourceKind) -> &'static str {
    match kind {
        crate::SourceKind::LooseDir => "looseDir",
        crate::SourceKind::Archive => "archive",
    }
}

pub(crate) fn source_kind_from_name(name: &str) -> Result<crate::SourceKind> {
    match name {
        "looseDir" => Ok(crate::SourceKind::LooseDir),
        "archive" => Ok(crate::SourceKind::Archive),
        other => Err(Error::runtime(format!(
            "dream.vfs: unknown source kind '{other}' (expected 'looseDir' or 'archive')"
        ))),
    }
}

/// The array of byte strings at the argument `view`, as host paths. `what` names the argument
/// in errors (`dream.vfs: dirs: Lua stack index 1: expected table, got number`, `dream.vfs:
/// dirs[2]: expected string, got number`).
pub(crate) fn paths_from_array(
    scope: &impl Scope,
    view: ValueView<'_>,
    what: &str,
) -> Result<Vec<PathBuf>> {
    let table = view
        .as_table()
        .map_err(|error| Error::runtime(format!("dream.vfs: {what}: {error}")))?;
    scope
        .with_frame(|frame| paths_from_table(frame, &table, what))
        .map_err(|error| Error::runtime(format!("dream.vfs: {error}")))
}

/// The array of byte strings in `table`, as host paths, walked one element at a time. `path`
/// names the table in an element's type error (`dirs[2]: expected string, got number`, Luau's own
/// type name, as the binder's conversions word theirs); under
/// an option reader it is the field's full path (`format!("{}.files", o.context())`), so the
/// reader adds nothing and the error reads `LayerIndex.fromFileLists[1].files[2]: ...`.
pub(crate) fn paths_from_table(
    frame: &Frame<'_>,
    table: &TableView<'_>,
    path: &str,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::with_capacity(table.raw_len());
    table.for_each_array(frame, |_, index, item| {
        if !item.is_string() {
            return Err(item.field_type_error_of(&format!("{path}[{index}]"), Type::String));
        }
        paths.push(host_path(item.read::<&[u8]>()?));
        Ok(())
    })?;
    Ok(paths)
}

/// Binds `callable` as `name` inside a class table, with a debug name under this extension.
pub(crate) fn class_function<F: l3i::bind::Binding<M>, M>(
    runtime: &l3i::Runtime,
    table: &Table,
    class: &str,
    name: &str,
    callable: F,
) -> Result<()> {
    let function: Function =
        runtime.bind_function(&format!("dream.vfs.{class}.{name}"), callable)?;
    table.set(&runtime.stack(), name, &function)
}

/// A new class table, frozen once `fill` has bound its functions.
pub(crate) fn frozen_class_table(
    runtime: &l3i::Runtime,
    fill: impl FnOnce(&Table) -> Result<()>,
) -> Result<Table> {
    let table = Table::new(&runtime.stack(), 0, 4)?;
    fill(&table)?;
    l3i::readonly::make_read_only(runtime, &table)?;
    Ok(table)
}

#[cfg(test)]
mod tests;
