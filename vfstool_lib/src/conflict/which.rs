// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::{VFS, normalize_path_in_place, reports::WhichResult};
use std::path::{Path, PathBuf};

impl ConflictIndex {
    /// Determine which source wins for `path` and which others also contain it.
    ///
    /// Returns `None` if `path` is not in the VFS at all. When the file exists
    /// in only one source, `also_in` is empty and `is_unique` is `true`.
    #[must_use]
    pub fn which(&self, vfs: &VFS, path: &Path) -> Option<WhichResult> {
        let winner = vfs.get_file(path)?;

        let winner_display = if winner.is_loose() {
            winner.path().display().to_string()
        } else {
            winner.parent_archive_path().unwrap_or_default()
        };

        let mut normalized = path.to_path_buf();
        normalize_path_in_place(&mut normalized);
        let source_indices = self.sources_containing(&normalized);

        let winner_src_idx = if winner.is_loose() {
            self.source_idx_for_loose_file(Some(&normalized), winner.path())
        } else {
            winner
                .parent_archive_path()
                .and_then(|ap| self.source_idx_for_archive_path(&ap))
        };

        let also_in: Vec<PathBuf> = source_indices
            .iter()
            .filter(|&&idx| Some(idx) != winner_src_idx)
            .map(|&idx| self.sources[idx].clone())
            .collect();

        let is_unique = source_indices.is_empty();

        Some(WhichResult {
            winner: winner_display,
            also_in,
            is_unique,
        })
    }
}
