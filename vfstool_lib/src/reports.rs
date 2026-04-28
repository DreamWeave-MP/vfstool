// SPDX-License-Identifier: GPL-3.0-only
//! Report types returned by the conflict, shadowed, provider, and diff subcommands.
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

/// Report listing sources whose files are entirely overridden by higher-priority sources.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ShadowedReport {
    /// Fully shadowed sources.
    pub sources: Vec<ShadowedSource>,
}

/// A single source whose files are all shadowed by higher-priority sources.
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ShadowedSource {
    /// Absolute path to the source directory or archive.
    pub path: PathBuf,
    /// VFS paths of all files in this source, each overridden by a later source.
    ///
    /// This is empty for summary reports built without file listings.
    #[cfg_attr(feature = "serialize", serde(skip_serializing_if = "Vec::is_empty"))]
    pub shadowed_files: Vec<PathBuf>,
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
