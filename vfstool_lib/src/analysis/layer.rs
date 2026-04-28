// SPDX-License-Identifier: GPL-3.0-only
use super::{
    LayerIndex, LayerProvider, SourceContribution, SourceContributionReport, SourceKind, SourceMeta,
};
use crate::{
    NormalizedKey, NormalizedPath, SourceId, VfsKeyInput,
    paths::{key_to_path_buf_lossy, normalized_safe_key},
};
use ahash::AHashMap;
use std::path::{Path, PathBuf};

impl LayerIndex {
    /// Build a provider index from ordered `(source_meta, paths)` pairs.
    ///
    /// Paths are normalized and unsafe materialization keys (absolute paths, parent traversal,
    /// prefixes, and empty keys) are skipped. Lower source indices have lower priority.
    pub fn from_file_lists(sources: impl IntoIterator<Item = (SourceMeta, Vec<PathBuf>)>) -> Self {
        let mut source_paths = Vec::new();
        let mut path_to_sources: AHashMap<NormalizedKey, Vec<usize>> = AHashMap::new();
        let mut provider_paths: AHashMap<(usize, NormalizedKey), Vec<PathBuf>> = AHashMap::new();

        for (source_meta, files) in sources {
            let idx = source_paths.len();
            source_paths.push(source_meta);
            for path in files {
                let Some(normalized_path) = normalized_safe_key(&path) else {
                    continue;
                };
                let key = NormalizedKey::from(normalized_path);
                provider_paths
                    .entry((idx, key.clone()))
                    .or_default()
                    .push(path);
                path_to_sources.entry(key).or_default().push(idx);
            }
        }

        Self {
            sources: source_paths,
            path_to_sources,
            provider_paths,
        }
    }

    /// Returns all normalized keys in sorted order.
    #[must_use]
    pub fn keys(&self) -> Vec<NormalizedPath> {
        let mut keys: Vec<NormalizedPath> = self
            .path_to_sources
            .keys()
            .map(|key| key.as_normalized_path().clone())
            .collect();
        keys.sort();
        keys
    }

    /// Returns the stable source ID for `path` if present.
    #[must_use]
    pub fn source_id_for_path(&self, path: &Path) -> Option<SourceId> {
        self.sources
            .iter()
            .position(|meta| meta.path == path)
            .map(SourceId::from_index)
    }

    /// Returns source metadata for a stable source ID.
    #[must_use]
    pub fn source_by_id(&self, source_id: SourceId) -> Option<&SourceMeta> {
        self.sources.get(source_id.as_index())
    }

    /// Returns source indices that provide `path`, in load order.
    pub fn sources_containing<K: VfsKeyInput + ?Sized>(&self, path: &K) -> &[usize] {
        let normalized = NormalizedKey::from(path.to_vfs_key());
        self.path_to_sources
            .get(&normalized)
            .map_or(&[], Vec::as_slice)
    }

    /// Returns the original provider path recorded for `source_index` and `path`.
    #[must_use]
    pub fn provider_original_path<K: VfsKeyInput + ?Sized>(
        &self,
        source_index: usize,
        path: &K,
    ) -> Option<&Path> {
        let normalized = NormalizedKey::from(path.to_vfs_key());
        self.provider_paths
            .get(&(source_index, normalized))
            .and_then(|paths| paths.first())
            .map(PathBuf::as_path)
    }

    /// Returns the provider chain for `path` in low-to-high priority order.
    #[must_use]
    pub fn provider_chain(&self, path: &Path) -> Vec<LayerProvider> {
        let key = NormalizedPath::new(path.as_os_str().as_encoded_bytes());
        let mut occurrences_by_source: AHashMap<usize, usize> = AHashMap::new();
        self.sources_containing(&key)
            .iter()
            .enumerate()
            .filter_map(|(provider_index, &source_index)| {
                let source = self.sources.get(source_index)?.clone();
                let source_occurrence = occurrences_by_source.entry(source_index).or_default();
                let original_path = self
                    .provider_paths
                    .get(&(source_index, NormalizedKey::from(key.clone())))
                    .and_then(|paths| paths.get(*source_occurrence))
                    .map_or_else(|| key_to_path_buf_lossy(&key), PathBuf::from);
                *source_occurrence += 1;
                Some(LayerProvider {
                    source_index,
                    provider_index,
                    source,
                    key: key_to_path_buf_lossy(&key),
                    original_path,
                })
            })
            .collect()
    }

    /// Returns all keys with more than one provider, sorted by normalized key.
    #[must_use]
    pub fn duplicate_keys(&self) -> Vec<NormalizedPath> {
        self.keys()
            .into_iter()
            .filter(|key| self.sources_containing(key).len() > 1)
            .collect()
    }

    /// Returns contribution counts for every source in low-to-high priority order.
    #[must_use]
    pub fn source_contributions(&self) -> SourceContributionReport {
        let mut contributions: Vec<SourceContribution> = self
            .sources
            .iter()
            .cloned()
            .enumerate()
            .map(|(source_index, source)| SourceContribution {
                source_index,
                source,
                winning_files: 0,
                overriding_files: 0,
                overridden_files: 0,
                unique_files: 0,
                duplicate_files: 0,
                loose_files: 0,
                archive_files: 0,
            })
            .collect();

        for source_indices in self.path_to_sources.values() {
            let Some(winner_position) = source_indices.len().checked_sub(1) else {
                continue;
            };
            let is_unique = source_indices.len() == 1;
            for (position, &source_index) in source_indices.iter().enumerate() {
                if let Some(row) = contributions.get_mut(source_index) {
                    match row.source.kind {
                        SourceKind::LooseDir => row.loose_files += 1,
                        SourceKind::Archive => row.archive_files += 1,
                    }
                    if is_unique {
                        row.unique_files += 1;
                    } else {
                        row.duplicate_files += 1;
                    }
                    if position == winner_position {
                        row.winning_files += 1;
                    } else if source_indices[position + 1..]
                        .iter()
                        .any(|later| *later != source_index)
                    {
                        row.overridden_files += 1;
                    }
                    if source_indices[..position]
                        .iter()
                        .any(|earlier| *earlier != source_index)
                    {
                        row.overriding_files += 1;
                    }
                }
            }
        }

        SourceContributionReport {
            sources: contributions,
        }
    }
}
