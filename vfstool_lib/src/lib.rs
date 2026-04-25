// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(missing_docs)]
//! Virtual file system library for `OpenMW` modding tools.
/// Higher-level analysis APIs: provenance, semantic conflicts, lock manifests, and simulations.
pub mod analysis;
/// Low-level archive loading and enumeration (BSA, BA2, ZIP, PK3).
#[cfg(any(feature = "bsa", feature = "zip"))]
pub mod archives;
/// Conflict analysis: per-source override and overridden-by sets.
pub mod conflict;
/// Tree node used for display and serialization of VFS directory structure.
pub mod directory_node;
/// Core shared identifiers and normalized key/digest types.
pub mod foundation;
/// Conflict fingerprint knowledge base types and storage.
#[doc(hidden)]
pub mod kb;
/// Shared glob/path matching utilities.
pub mod matchers;
/// Provider-aware mutable VFS that can reveal lower-priority providers after removals.
pub mod mutable_vfs;
/// Path normalization and safety helpers.
pub mod paths;
/// Declarative policy rules and evaluation against VFS/layer state.
#[doc(hidden)]
pub mod policy;
/// Report types returned by conflict, shadowed, which, stats, and diff subcommands.
pub mod reports;
/// Utilities for the MO2-style `run` workflow: dump, snapshot, and finalize.
pub mod run;
/// Asset-class semantic analyzers and deltas.
#[doc(hidden)]
pub mod semantic;
/// Constraint-based load-order solving.
#[doc(hidden)]
pub mod solve;
/// Core [`VFS`] struct and directory-construction logic.
pub mod vfs;
/// [`VfsFile`] wrapper for loose and archive-backed files.
pub mod vfs_file;

pub use analysis::{
    DriftEntry, DriftKind, DriftReport, LayerIndex, SourceKind, SourceMeta, VfsLock, VfsLockEntry,
};
pub use conflict::{ConflictIndex, SourceConflicts};
pub(crate) use directory_node::DirectoryNode;
pub use foundation::{ContentDigest, NormalizedKey, SourceId};
pub use matchers::{path_glob_matches, source_glob_matches};
pub use mutable_vfs::{MutableVfs, VfsProvider};
pub use paths::{normalize_path, normalize_path_in_place};
pub use reports::{
    CollapseOptions, ConflictSourceEntry, ConflictsReport, DiffReport, ShadowedReport,
    ShadowedSource, StatsReport, StatsRow, WhichResult,
};
pub use run::{Snapshot, changed_files, run_finalize, run_setup, snapshot_directory};
pub use vfs::{DirectoryDiff, VFS};
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
