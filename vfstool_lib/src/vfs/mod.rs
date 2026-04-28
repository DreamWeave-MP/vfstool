// SPDX-License-Identifier: GPL-3.0-only
use ahash::AHashMap;

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
    ArchiveEntry, ArchiveInfo, CaseCollision, CaseCollisionReport, DuplicateEntry, DuplicateReport,
    ExplainReport, MaterializationAction, MaterializationIssue, MaterializationPlan,
    ValidationIssue, ValidationReport, VfsProviderRecord,
};

use crate::{LayerIndex, NormalizedPath, VfsFile};

// Owned
type MaybeFile<'a> = Option<&'a VfsFile>;
type VFSTuple<'a> = (&'a NormalizedPath, &'a VfsFile);
type VFSFiles = AHashMap<NormalizedPath, VfsFile>;

/// Virtual file system built from an ordered list of data directories and optional archives.
///
/// Keys are normalized (lowercase, forward-slash) relative paths. Later directories and loose
/// files have higher priority, matching `OpenMW`'s `data=` semantics.
pub struct VFS {
    file_map: VFSFiles,
    layer_index: LayerIndex,
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
