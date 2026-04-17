// SPDX-License-Identifier: MIT OR Apache-2.0
use std::path::PathBuf;

// --- Collapse ---

pub struct CollapseOptions {
    /// Fall back to copying when hardlinking fails.
    pub allow_copying: bool,
    /// Extract files from BSA/BA2 archives (write their contents as loose files).
    pub extract_archives: bool,
    /// Use symbolic links instead of hard links (allows cross-device linking).
    pub use_symlinks: bool,
}

// --- Conflicts ---

#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ConflictsReport {
    pub sources: Vec<ConflictSourceEntry>,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ConflictSourceEntry {
    pub path: PathBuf,
    pub overrides: Vec<PathBuf>,
    pub overridden_by: Vec<PathBuf>,
}

// --- Shadowed ---

#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ShadowedReport {
    pub sources: Vec<ShadowedSource>,
}

#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ShadowedSource {
    pub path: PathBuf,
    pub shadowed_files: Vec<PathBuf>,
}

// --- Which ---

pub struct WhichResult {
    /// Display path for the winning file (archive path for archived files, absolute path for loose).
    pub winner: String,
    /// Other source directories/archives that also contain this file (lower priority — overridden).
    pub also_in: Vec<PathBuf>,
    /// True when no other sources contain this file.
    pub is_unique: bool,
}

// --- Stats ---

pub struct StatsRow {
    pub source: PathBuf,
    pub wins: usize,
    pub overrides: usize,
    pub overridden: usize,
}

pub struct StatsReport {
    pub rows: Vec<StatsRow>,
}

// --- Diff ---

#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct DiffReport {
    pub source_a: PathBuf,
    pub source_b: PathBuf,
    pub higher_priority: PathBuf,
    pub shared: Vec<PathBuf>,
    pub only_in_a: Vec<PathBuf>,
    pub only_in_b: Vec<PathBuf>,
}
