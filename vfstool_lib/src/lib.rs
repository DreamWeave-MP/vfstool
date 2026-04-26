// SPDX-License-Identifier: MIT OR Apache-2.0
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
//! - [`LayerIndex`], [`VfsLock`], [`DriftReport`], and related types for provenance, lock, and
//!   drift workflows.
//! - [`run_setup`], [`run_finalize`], [`snapshot_directory`], and [`changed_files`] for
//!   dump-run-collect workflows.
//! - [`normalize_path`] and [`normalize_path_in_place`] for matching the library's key semantics.
//!
//! Modules marked `#[doc(hidden)]` remain public so the workspace can compose and test
//! experimental analyzers, policies, and solver code. They are not part of the promoted 1.0 API.
//! Depending on them is possible, but it is buying the sharp end of the rake intentionally.
//!
//! # Mutation model
//!
//! [`VFS`] mutators operate on the materialized winner map only. Removing a key removes it from
//! the resolved view; it does not reveal a lower-priority provider. Use [`MutableVfs`] if removal
//! should expose the next provider in the low-to-high priority stack.
//!
//! # Feature flags
//!
//! - `bsa`: BSA/BA2 archive support.
//! - `zip`: ZIP/PK3 archive support.
//! - `serialize`: JSON/YAML/TOML serialization and structured JSON/TOML semantic comparison.
//!   Without `serialize`, JSON and TOML semantic deltas are reported as unknown rather than parsed.
//!
//! # Runner warning
//!
//! [`run_setup`] may create hardlinks by default. Child tools that edit files in place can mutate
//! original loose source files through those hardlinks. Use copy mode for tools that are not
//! hardlink-safe. This is not a hidden safety feature; it is a tradeoff with teeth.
/// Higher-level analysis APIs: provenance, semantic conflicts, lock manifests, and simulations.
pub mod analysis;
/// Low-level archive loading and enumeration (BSA, BA2, ZIP, PK3).
#[cfg(any(feature = "bsa", feature = "zip"))]
pub mod archives;
/// Conflict analysis: per-source override and overridden-by sets.
pub mod conflict;
/// Tree node used for display and serialization of VFS directory structure.
pub mod directory_node;
/// Experimental analyzers, policies, solver, and knowledge-base helpers.
///
/// Prefer this namespace for unstable helpers. The hidden top-level modules below remain public
/// for compatibility, but they are not promoted 1.0 API. Yes, hidden public modules are still
/// public; pretending otherwise would just be documentation cosplay.
#[doc(hidden)]
pub mod experimental;
/// Core shared identifiers and normalized key/digest types.
pub mod foundation;
/// Conflict fingerprint knowledge base types and storage.
///
/// Unstable compatibility path; prefer [`experimental::kb`].
#[doc(hidden)]
pub mod kb;
/// Shared glob/path matching utilities.
pub mod matchers;
/// Provider-aware mutable VFS that can reveal lower-priority providers after removals.
pub mod mutable_vfs;
/// Path normalization and safety helpers.
pub mod paths;
/// Declarative policy rules and evaluation against VFS/layer state.
///
/// Unstable compatibility path; prefer [`experimental::policy`].
#[doc(hidden)]
pub mod policy;
/// Report types returned by conflict, shadowed, which, stats, and diff subcommands.
pub mod reports;
/// Utilities for the MO2-style `run` workflow: dump, snapshot, and finalize.
pub mod run;
mod semantic;
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
