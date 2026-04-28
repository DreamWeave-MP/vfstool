// SPDX-License-Identifier: GPL-3.0-only
use super::ConflictIndex;
use crate::{VFS, paths::key_to_string_lossy, reports::DiffReport};
use std::{collections::HashSet, path::Path};

impl ConflictIndex {
    /// Compare two data directories: which files are shared, unique to each,
    /// and which has higher load-order priority.
    #[must_use]
    pub fn diff_report(&self, source_a: &Path, source_b: &Path) -> DiffReport {
        let vfs_a = VFS::from_directories([source_a], None);
        let vfs_b = VFS::from_directories([source_b], None);

        let keys_a: HashSet<String> = vfs_a.iter().map(|(k, _)| key_to_string_lossy(k)).collect();
        let keys_b: HashSet<String> = vfs_b.iter().map(|(k, _)| key_to_string_lossy(k)).collect();

        let mut shared: Vec<String> = keys_a.intersection(&keys_b).cloned().collect();
        let mut only_in_a: Vec<String> = keys_a.difference(&keys_b).cloned().collect();
        let mut only_in_b: Vec<String> = keys_b.difference(&keys_a).cloned().collect();
        shared.sort();
        only_in_a.sort();
        only_in_b.sort();

        let idx_a = self.sources.iter().position(|s| s == source_a);
        let idx_b = self.sources.iter().position(|s| s == source_b);
        let higher_priority = match (idx_a, idx_b) {
            (Some(a), Some(b)) => {
                if a > b {
                    source_a.to_path_buf()
                } else {
                    source_b.to_path_buf()
                }
            }
            _ => source_b.to_path_buf(),
        };

        DiffReport {
            source_a: source_a.to_path_buf(),
            source_b: source_b.to_path_buf(),
            higher_priority,
            shared,
            only_in_a,
            only_in_b,
        }
    }
}
