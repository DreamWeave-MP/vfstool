// SPDX-License-Identifier: MIT OR Apache-2.0
use super::VFS;
use crate::{VfsFile, normalize_path_in_place};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Result of scanning a directory against a [`VFS`].
///
/// Produced by [`VFS::diff_directory`]. Each scanned file falls into exactly
/// one of two categories:
///
/// - **`conflicts`** — the file exists in both the directory and the VFS.
///   Each entry pairs the incoming file (from the directory) with the file
///   currently in the VFS that it would displace.
///
/// - **`additions`** — the file exists only in the directory; it would be a
///   net-new entry in the VFS.
pub struct DirectoryDiff<'vfs> {
    /// Files present in both the directory and the VFS.
    /// `(normalized_key, incoming_file, current_vfs_entry)`.
    pub conflicts: Vec<(PathBuf, VfsFile, &'vfs VfsFile)>,

    /// Files in the directory that are not in the VFS.
    pub additions: Vec<(PathBuf, VfsFile)>,
}

impl VFS {
    /// Scans `dir` and classifies every file against this VFS.
    ///
    /// Each file found under `dir` falls into one of two categories:
    ///
    /// - **conflict** — a file at the same normalized path already exists in
    ///   the VFS. The returned entry pairs the incoming file with the VFS entry
    ///   it would displace.
    /// - **addition** — no file at that path exists in the VFS; installing this
    ///   directory would add it as a new entry.
    ///
    /// The directory is walked in parallel (rayon). Lookups against the VFS map
    /// are O(1) per file. This is the primitive for mod conflict analysis: call
    /// it once per candidate directory to get the full picture of what that mod
    /// installs, overrides, and adds.
    pub fn diff_directory<P: AsRef<Path> + Sync>(&self, dir: P) -> DirectoryDiff<'_> {
        let dir = dir.as_ref().to_path_buf();

        // Walk the directory in parallel — I/O is the bottleneck here.
        let entries: Vec<(PathBuf, VfsFile)> = WalkDir::new(&dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|entry| match entry {
                Ok(entry) if entry.file_type().is_file() => Some(entry),
                Ok(_) => None,
                Err(err) => {
                    eprintln!(
                        "vfstool: warning: failed to walk '{}': {err}",
                        dir.display()
                    );
                    None
                }
            })
            .par_bridge()
            .map(|entry| {
                let mut normalized = entry
                    .path()
                    .strip_prefix(&dir)
                    .map_or_else(|_| entry.path().to_path_buf(), PathBuf::from);
                normalize_path_in_place(&mut normalized);
                (normalized, VfsFile::from(entry.path()))
            })
            .collect();

        // Single O(1) HashMap lookup per entry — no VFS iteration required.
        let mut conflicts = Vec::new();
        let mut additions = Vec::new();

        for (key, incoming) in entries {
            match self.file_map.get(&key) {
                Some(existing) => conflicts.push((key, incoming, existing)),
                None => additions.push((key, incoming)),
            }
        }

        DirectoryDiff {
            conflicts,
            additions,
        }
    }
}
