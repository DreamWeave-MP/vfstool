// SPDX-License-Identifier: GPL-3.0-only
#![deny(missing_docs)]
//! Virtual file system library for `OpenMW` modding tools.
//!
//! `vfstool_lib` builds a resolved view of an `OpenMW`-style virtual file system from ordered
//! data directories and, when archive features are enabled, BSA/BA2/ZIP/PK3 archives. Paths are
//! normalized to lowercase keys with `/` separators. Directory priority follows `OpenMW`'s
//! `data=` semantics: later loose directories win, and loose files override archive entries.
//!
//! # Stable 1.0 surface
//!
//! Prefer the top-level re-exports for application code:
//!
//! - [`VFS`] for the resolved winner map and materialization helpers.
//! - [`MutableVfs`] and [`VfsProvider`] when provider stacks must survive winner removal.
//! - [`LayerIndex`] as the canonical provider-chain index and [`ConflictIndex`] as its derived
//!   conflict projection, plus reports such as [`ConflictsReport`], [`ShadowedReport`], and
//!   [`DiffReport`] for load-order diagnostics.
//! - [`LayerIndex`], [`VfsLock`], [`DriftReport`], and related types for provenance, lock, drift,
//!   and semantic conflict workflows.
//! - [`run_setup`], [`run_finalize`], [`snapshot_directory`], and [`changed_files`] for
//!   dump-run-collect workflows.
//! - [`normalize_path`] and [`normalize_path_in_place`] for matching the library's key semantics.
//!
//! [`experimental`] exposes policy helpers, solver code, and knowledge-base helpers that are
//! intentionally unstable. Depending on them is possible, but it is buying the sharp end of the rake
//! intentionally.
//!
//! # Mutation model
//!
//! [`VFS`] mutators operate on the materialized winner map only. Removing a key removes it from
//! the resolved view; it does not reveal a lower-priority provider. Use [`MutableVfs`] if removal
//! should expose the next provider in the low-to-high priority stack.
//!
//! # Examples
//!
//! Build a VFS from two data directories and query the winner. Later directories have higher
//! priority, so the second directory wins when both provide the same normalized key.
//!
//! ```
//! use std::{fs, path::Path};
//! use vfstool_lib::VFS;
//!
//! # fn unique_dir(name: &str) -> std::path::PathBuf {
//! #     let dir = std::env::temp_dir().join(format!(
//! #         "vfstool_doc_{name}_{}_{}",
//! #         std::process::id(),
//! #         std::time::SystemTime::now()
//! #             .duration_since(std::time::UNIX_EPOCH)
//! #             .unwrap()
//! #             .as_nanos()
//! #     ));
//! #     fs::create_dir_all(&dir).unwrap();
//! #     dir
//! # }
//! # let low = unique_dir("low");
//! # let high = unique_dir("high");
//! # fs::create_dir_all(low.join("Textures")).unwrap();
//! # fs::create_dir_all(high.join("textures")).unwrap();
//! # fs::write(low.join("Textures/Foo.DDS"), b"low").unwrap();
//! # fs::write(high.join("textures/foo.dds"), b"high").unwrap();
//! let vfs = VFS::from_directories([&low, &high], None);
//!
//! let winner = vfs.get_file("TEXTURES\\FOO.DDS").unwrap();
//! assert_eq!(winner.path(), high.join("textures/foo.dds"));
//! assert!(vfs.contains(Path::new("textures/foo.dds")));
//! # let _ = fs::remove_dir_all(low);
//! # let _ = fs::remove_dir_all(high);
//! ```
//!
//! Ask the provider index why a key resolves the way it does.
//!
//! ```
//! use std::path::{Path, PathBuf};
//! use vfstool_lib::{LayerIndex, SourceKind, SourceMeta};
//!
//! let layer = LayerIndex::from_file_lists([
//!     (
//!         SourceMeta { path: PathBuf::from("base"), kind: SourceKind::LooseDir },
//!         vec![PathBuf::from("textures/foo.dds")],
//!     ),
//!     (
//!         SourceMeta { path: PathBuf::from("mod"), kind: SourceKind::LooseDir },
//!         vec![PathBuf::from("textures/foo.dds")],
//!     ),
//! ]);
//!
//! let chain = layer.provider_chain(Path::new("textures/foo.dds"));
//! assert_eq!(chain.len(), 2);
//! assert_eq!(chain.last().unwrap().source.path, PathBuf::from("mod"));
//! ```
//!
//! Use semantic analysis for modest, content-aware comparisons. This is not clairvoyance; it is
//! deliberately scoped classification for formats the library understands.
//!
//! ```
//! use vfstool_lib::{analyze_pair, AssetClass, SemanticDelta};
//!
//! let (class, delta) = analyze_pair(
//!     std::path::Path::new("settings.ini"),
//!     b"[section]\na = 1\nb = 2\n",
//!     b"# reordered\n[section]\nb = 2\na = 1\n",
//! );
//!
//! assert_eq!(class, AssetClass::Ini);
//! assert_eq!(delta, SemanticDelta::CosmeticOnly);
//! ```
//!
//! # Feature flags
//!
//! - `bsa`: BSA/BA2 archive support.
//! - `zip`: ZIP/PK3 archive support.
//! - `serialize`: JSON/YAML/TOML serialization and structured JSON/TOML semantic comparison.
//!   Without `serialize`, JSON and TOML semantic deltas are reported as unknown rather than parsed.
//! - `lua`: embedded `mlua` bindings for the promoted stable API surface. This is not a `cdylib`
//!   Lua module; hosts register `lua::open` or `lua::register` into their own Lua state.
//!
//! # Runner warning
//!
//! [`run_setup`] may create hardlinks by default. Child tools that edit files in place can mutate
//! original loose source files through those hardlinks. Use copy mode for tools that are not
//! hardlink-safe. This is not a hidden safety feature; it is a tradeoff with teeth.
/// Higher-level analysis APIs: provenance, lock manifests, drift, and semantic conflict reports.
pub mod analysis;
/// Low-level archive loading and enumeration (BSA, BA2, ZIP, PK3).
#[cfg(any(feature = "bsa", feature = "zip"))]
pub mod archives;
/// Conflict analysis: per-source override and overridden-by sets.
pub mod conflict;
/// Tree node used for display and serialization of VFS directory structure.
pub mod directory_node;
/// Experimental policies, solver, and knowledge-base helpers.
///
/// This namespace is public but unstable: useful for composing advanced workflows, not promoted as
/// frozen 1.0 API.
pub mod experimental;
/// Core shared identifiers and normalized key/digest types.
pub mod foundation;
mod kb;
/// Embedded Lua bindings for the promoted stable API surface.
#[cfg(feature = "lua")]
pub mod lua;
/// Shared glob/path matching utilities.
pub mod matchers;
/// Provider-aware mutable VFS that can reveal lower-priority providers after removals.
pub mod mutable_vfs;
/// Path normalization and safety helpers.
pub mod paths;
mod policy;
/// Report types returned by conflict, shadowed, provider, and diff subcommands.
pub mod reports;
/// Utilities for the MO2-style `run` workflow: dump, snapshot, and finalize.
pub mod run;
/// Semantic analyzers and semantic conflict report types.
pub mod semantic;
mod solve;
/// Core [`VFS`] struct and directory-construction logic.
pub mod vfs;
/// [`VfsFile`] wrapper for loose and archive-backed files.
pub mod vfs_file;

pub use analysis::{
    DriftEntry, DriftKind, DriftReport, LayerIndex, LayerProvider, SourceContribution,
    SourceContributionReport, SourceKind, SourceMeta, VfsLock, VfsLockEntry,
};
pub use conflict::{ConflictIndex, SourceConflicts};
pub(crate) use directory_node::DirectoryNode;
pub use foundation::{ContentDigest, NormalizedKey, SourceId};
pub use matchers::{path_glob_matches, source_glob_matches};
pub use mutable_vfs::{MutableVfs, VfsProvider};
pub use paths::{normalize_path, normalize_path_in_place};
pub use reports::{
    CollapseOptions, ConflictSourceEntry, ConflictsReport, DiffReport, ShadowedReport,
    ShadowedSource,
};
pub use run::{
    MetadataSnapshot, Snapshot, SnapshotEntry, changed_files, changed_files_metadata, run_finalize,
    run_finalize_tracked, run_setup, run_setup_tracked, snapshot_directory,
    snapshot_directory_metadata,
};
pub use semantic::{
    ArchiveHashMode, AssetClass, SemanticConflict, SemanticConflictReport, SemanticDelta,
    SemanticOpts, SemanticProvider, SemanticRelation, analyze_pair,
};
pub use vfs::{
    ArchiveEntry, ArchiveInfo, CaseCollision, CaseCollisionReport, DirectoryDiff, DuplicateEntry,
    DuplicateReport, ExplainReport, MaterializationAction, MaterializationIssue,
    MaterializationPlan, VFS, ValidationIssue, ValidationReport, VfsProviderRecord,
};
pub use vfs_file::VfsFile;

use std::{collections::BTreeMap, path::PathBuf};

/// Sorted map from a directory name to its [`DirectoryNode`], used for display and serialization.
pub type DisplayTree = BTreeMap<PathBuf, DirectoryNode>;

/// Output format for [`serialize_value`] and [`VFS::serialize_from_tree`].
#[derive(Debug, Clone, Copy)]
pub enum SerializeType {
    /// Serialize as JSON.
    Json,
    /// Serialize as YAML.
    Yaml,
    /// Serialize as TOML.
    Toml,
}

/// Serialize any `serde::Serialize` value to JSON, YAML, or TOML.
#[cfg(feature = "serialize")]
///
/// # Errors
///
/// Returns an error if serialization to the requested format fails.
pub fn serialize_value<T: serde::Serialize>(
    value: &T,
    write_type: SerializeType,
) -> std::io::Result<String> {
    fn to_io_error<E: std::fmt::Display>(err: E) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
    }
    match write_type {
        SerializeType::Json => serde_json::to_string(value).map_err(to_io_error),
        SerializeType::Yaml => serde_yaml::to_string(value).map_err(to_io_error),
        SerializeType::Toml => toml::to_string_pretty(value).map_err(to_io_error),
    }
}
