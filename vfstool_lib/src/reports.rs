// SPDX-License-Identifier: MIT OR Apache-2.0
//! Report types returned by the conflict, shadowed, which, stats, and diff subcommands.
use std::path::PathBuf;

// --- Collapse ---

/// Options that control how [`VFS::collapse_into`](crate::vfs::VFS::collapse_into)
/// links or copies files.
pub struct CollapseOptions {
    /// Fall back to copying when hardlinking fails.
    pub allow_copying: bool,
    /// Extract files from BSA/BA2 archives (write their contents as loose files).
    pub extract_archives: bool,
    /// Use symbolic links instead of hard links (allows cross-device linking).
    pub use_symlinks: bool,
}

// --- Conflicts ---

/// Full conflicts report listing every source's overrides and overridden-by files.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ConflictsReport {
    /// Per-source conflict entries, in load-order position.
    pub sources: Vec<ConflictSourceEntry>,
}

/// Conflict information for a single source directory or archive.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ConflictSourceEntry {
    /// Absolute path to the source directory or archive.
    pub path: PathBuf,
    /// VFS paths where this source overrides at least one earlier (lower-priority) source.
    pub overrides: Vec<PathBuf>,
    /// VFS paths where this source is overridden by at least one later (higher-priority) source.
    pub overridden_by: Vec<PathBuf>,
}

// --- Shadowed ---

/// Report listing every source that has at least one file overridden by a higher-priority source.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ShadowedReport {
    /// Sources with one or more shadowed (overridden) files.
    pub sources: Vec<ShadowedSource>,
}

/// A single source whose files are (partially or fully) shadowed by higher-priority sources.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ShadowedSource {
    /// Absolute path to the source directory or archive.
    pub path: PathBuf,
    /// VFS paths of files in this source that are overridden by a later source.
    pub shadowed_files: Vec<PathBuf>,
}

// --- Which ---

/// Result of a `which` query: the winning file and any lower-priority sources that also have it.
pub struct WhichResult {
    /// Display path for the winning file (archive path for archived files, absolute path for loose).
    pub winner: String,
    /// Other source directories/archives that also contain this file (lower priority — overridden).
    pub also_in: Vec<PathBuf>,
    /// True when no other sources contain this file.
    pub is_unique: bool,
}

// --- Stats ---

/// Win/override/overridden counts for a single source in the load order.
pub struct StatsRow {
    /// Absolute path to the source directory or archive.
    pub source: PathBuf,
    /// Number of VFS files currently served from this source (it "won" on these paths).
    pub wins: usize,
    /// Number of files where this source overrides at least one earlier source.
    pub overrides: usize,
    /// Number of files where this source is overridden by at least one later source.
    pub overridden: usize,
}

/// Aggregated win/conflict statistics across all sources in the load order.
pub struct StatsReport {
    /// One row per source, in load-order position.
    pub rows: Vec<StatsRow>,
}

// --- Diff ---

/// Comparison report between two source directories.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct DiffReport {
    /// Absolute path to the first source directory.
    pub source_a: PathBuf,
    /// Absolute path to the second source directory.
    pub source_b: PathBuf,
    /// Whichever of `source_a` or `source_b` has higher load-order priority.
    pub higher_priority: PathBuf,
    /// VFS paths present in both sources.
    pub shared: Vec<PathBuf>,
    /// VFS paths present only in `source_a`.
    pub only_in_a: Vec<PathBuf>,
    /// VFS paths present only in `source_b`.
    pub only_in_b: Vec<PathBuf>,
}
