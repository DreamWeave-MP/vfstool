// SPDX-License-Identifier: GPL-3.0-only
use ahash::AHashMap;
use std::sync::OnceLock;

mod build;
mod construct;
mod core;
mod diff;
mod lookup;
mod materialize;
mod mutate;
mod providers;
mod tree;

pub use self::diff::DirectoryDiff;
pub use self::providers::{
    ArchiveEntry, ArchiveInfo, DuplicateEntry, DuplicateReport, ExplainReport,
    MaterializationAction, MaterializationIssue, MaterializationPlan, VfsProviderRecord,
};

use crate::{LayerIndex, NormalizedPath, SourceMeta, VfsFile};

// Owned
type MaybeFile<'a> = Option<&'a VfsFile>;
type VFSTuple<'a> = (&'a NormalizedPath, &'a VfsFile);
type VFSFiles = AHashMap<NormalizedPath, VfsFile>;

/// One provider for a normalized VFS key.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VfsProvider {
    /// Source metadata for the provider.
    pub source: SourceMeta,
    /// Backing file for this provider.
    pub file: VfsFile,
}

impl VfsProvider {
    /// Create a VFS provider from source metadata and backing file.
    #[must_use]
    pub fn new(source: SourceMeta, file: VfsFile) -> Self {
        Self { source, file }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderEntry {
    pub(crate) source_index: usize,
    pub(crate) provider: VfsProvider,
}

/// Virtual file system built from an ordered list of data directories and optional archives.
///
/// Keys are normalized (lowercase, forward-slash) relative paths. Later directories and loose
/// files have higher priority, matching `OpenMW`'s `data=` semantics.
pub struct VFS {
    file_map: VFSFiles,
    dir_prefix_counts: AHashMap<NormalizedPath, usize>,
    pub(crate) providers: AHashMap<NormalizedPath, Vec<ProviderEntry>>,
    pub(crate) sources: Vec<SourceMeta>,
    layer_index: OnceLock<LayerIndex>,
}

#[cfg(test)]
#[path = "tests/dump_tests.rs"]
mod dump_tests;
#[cfg(test)]
#[path = "tests/loose_tests.rs"]
mod loose_tests;
#[cfg(all(test, feature = "beth-archives"))]
#[path = "tests/tests.rs"]
mod tests;
#[cfg(all(test, feature = "zip"))]
#[path = "tests/zip_tests.rs"]
mod zip_tests;
