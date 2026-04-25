// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::normalize_path;
use std::path::Path;

impl ConflictIndex {
    pub(super) fn source_idx_for_loose_file(
        &self,
        key: Option<&Path>,
        path: &Path,
    ) -> Option<usize> {
        if let Some(key) = key {
            let normalized_path = normalize_path(path);
            if let Some((idx, _)) = self.sources.iter().enumerate().find(|(_, src)| {
                normalize_path(&src.join(key)).as_ref() == normalized_path.as_ref()
            }) {
                return Some(idx);
            }
        }

        self.sources
            .iter()
            .enumerate()
            .filter(|(_, src)| path.starts_with(src))
            .max_by_key(|(_, src)| src.components().count())
            .map(|(idx, _)| idx)
    }

    pub(super) fn source_idx_for_archive_path(&self, archive_path: &str) -> Option<usize> {
        let archive_path = normalize_path(Path::new(archive_path));
        self.sources
            .iter()
            .position(|src| normalize_path(src).as_ref() == archive_path.as_ref())
    }
}
