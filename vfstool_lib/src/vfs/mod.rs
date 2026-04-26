// SPDX-License-Identifier: MIT OR Apache-2.0
use ahash::AHashMap;

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
    SourceContribution, SourceContributionReport, ValidationIssue, ValidationReport,
    VfsProviderRecord,
};

use crate::VfsFile;
use std::path::{Path, PathBuf};

// Owned
type MaybeFile<'a> = Option<&'a VfsFile>;
type VFSTuple<'a> = (&'a Path, &'a VfsFile);
type VFSFiles = AHashMap<PathBuf, VfsFile>;

/// Virtual file system built from an ordered list of data directories and optional archives.
///
/// Keys are normalized (lowercase, forward-slash) relative paths. Later directories and loose
/// files have higher priority, matching `OpenMW`'s `data=` semantics.
pub struct VFS {
    file_map: VFSFiles,
    provider_index: providers::VfsProviderIndex,
}

#[cfg(test)]
#[path = "tests/dump_tests.rs"]
mod dump_tests;
#[cfg(test)]
#[path = "tests/loose_tests.rs"]
mod loose_tests;
#[cfg(all(test, feature = "bsa"))]
#[path = "tests/tests.rs"]
mod tests;
#[cfg(all(test, feature = "zip"))]
#[path = "tests/zip_tests.rs"]
mod zip_tests;
