// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::{
    VFS,
    reports::{StatsReport, StatsRow},
};

impl ConflictIndex {
    /// Compute per-source win/override/overridden counts.
    ///
    /// "Wins" is the number of VFS files served from that source (i.e., it has
    /// the highest priority for those files). "Overrides" and "overridden" come
    /// directly from the conflict sets.
    #[must_use]
    pub fn stats(&self, vfs: &VFS) -> StatsReport {
        let mut wins: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for (key, file) in vfs.iter() {
            let source_idx = if file.is_loose() {
                self.source_idx_for_loose_file(Some(key), file.path())
            } else {
                file.parent_archive_path()
                    .and_then(|ap| self.source_idx_for_archive_path(&ap))
            };
            if let Some(idx) = source_idx {
                *wins.entry(idx).or_insert(0) += 1;
            }
        }

        let rows = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, src)| StatsRow {
                source: src.clone(),
                wins: wins.get(&i).copied().unwrap_or(0),
                overrides: self.conflicts[i].overrides.len(),
                overridden: self.conflicts[i].overridden_by.len(),
            })
            .collect();

        StatsReport { rows }
    }
}
