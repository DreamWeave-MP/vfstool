// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{LayerIndex, SourceMeta};
use crate::{NormalizedKey, SourceId};
use ahash::{AHashMap, AHashSet};
use std::path::{Path, PathBuf};

impl LayerIndex {
    /// Build a provider index from ordered `(source_meta, normalized_paths)` pairs.
    pub fn from_file_lists(sources: impl IntoIterator<Item = (SourceMeta, Vec<PathBuf>)>) -> Self {
        let mut source_paths = Vec::new();
        let mut path_to_sources: AHashMap<NormalizedKey, Vec<usize>> = AHashMap::new();
        let mut provider_paths: AHashMap<(usize, NormalizedKey), PathBuf> = AHashMap::new();

        for (source_meta, files) in sources {
            let idx = source_paths.len();
            source_paths.push(source_meta);
            let mut seen = AHashSet::new();

            for path in files {
                let key = NormalizedKey::new(&path);
                if seen.insert(key.clone()) {
                    provider_paths.insert((idx, key.clone()), path);
                    path_to_sources.entry(key).or_default().push(idx);
                }
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
    pub fn keys(&self) -> Vec<PathBuf> {
        let mut keys: Vec<PathBuf> = self
            .path_to_sources
            .keys()
            .cloned()
            .map(NormalizedKey::into_path_buf)
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
    pub fn sources_containing(&self, path: &Path) -> &[usize] {
        let normalized = NormalizedKey::new(path);
        self.path_to_sources
            .get(&normalized)
            .map_or(&[], Vec::as_slice)
    }

    /// Returns the original provider path recorded for `source_index` and `path`.
    pub(crate) fn provider_original_path(&self, source_index: usize, path: &Path) -> Option<&Path> {
        let normalized = NormalizedKey::new(path);
        self.provider_paths
            .get(&(source_index, normalized))
            .map(PathBuf::as_path)
    }

    /// Replace the provider chain for `key` with a single winner provider.
    pub(crate) fn set_single_provider(
        &mut self,
        key: &Path,
        source: SourceMeta,
        provider_path: PathBuf,
    ) {
        self.remove_key(key);
        let normalized = NormalizedKey::new(key);
        let source_index = self.sources.len();
        self.sources.push(source);
        self.path_to_sources
            .insert(normalized.clone(), vec![source_index]);
        self.provider_paths
            .insert((source_index, normalized), provider_path);
    }

    /// Remove all providers for `key` from the index.
    pub(crate) fn remove_key(&mut self, key: &Path) {
        let normalized = NormalizedKey::new(key);
        if let Some(source_indices) = self.path_to_sources.remove(&normalized) {
            for source_index in source_indices {
                self.provider_paths
                    .remove(&(source_index, normalized.clone()));
            }
        }
    }
}
