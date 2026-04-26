// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::normalize_path;
use ahash::AHashMap;
use std::path::{Path, PathBuf};

pub(super) struct SourceAttributionIndex {
    normalized_sources: Vec<(usize, PathBuf)>,
    loose_prefixes: Vec<(usize, PathBuf)>,
    archive_sources: AHashMap<PathBuf, usize>,
}

impl ConflictIndex {
    pub(super) fn source_attribution_index(&self) -> SourceAttributionIndex {
        let normalized_sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(idx, source)| (idx, normalize_path(source).into_owned()))
            .collect();

        let mut loose_prefixes: Vec<_> = self
            .sources
            .iter()
            .enumerate()
            .map(|(idx, source)| (idx, source.clone()))
            .collect();
        loose_prefixes.sort_by_key(|(_, source)| std::cmp::Reverse(source.components().count()));

        let archive_sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(idx, source)| (normalize_path(source).into_owned(), idx))
            .collect();

        SourceAttributionIndex {
            normalized_sources,
            loose_prefixes,
            archive_sources,
        }
    }

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

impl SourceAttributionIndex {
    pub(super) fn source_idx_for_loose_file(&self, key: &Path, path: &Path) -> Option<usize> {
        let normalized_path = normalize_path(path);
        let normalized_key = normalize_path(key);
        if let Some((idx, _)) = self
            .normalized_sources
            .iter()
            .find(|(_, source)| source.join(normalized_key.as_ref()) == normalized_path.as_ref())
        {
            return Some(*idx);
        }

        self.loose_prefixes
            .iter()
            .find(|(_, source)| path.starts_with(source))
            .map(|(idx, _)| *idx)
    }

    pub(super) fn source_idx_for_archive_path(&self, archive_path: &str) -> Option<usize> {
        self.archive_sources
            .get(normalize_path(Path::new(archive_path)).as_ref())
            .copied()
    }
}
