// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
use ahash::AHashMap;
use rayon::prelude::*;

use crate::{
    LayerIndex, NormalizedPath, SourceKind, SourceMeta, VfsFile,
    paths::{key_to_path_buf_bytes, key_to_path_buf_lossy},
};
use std::path::{Path, PathBuf};

impl VFS {
    pub(super) const DIR_PREFIX: &str = "├── ";
    pub(super) const FILE_PREFIX: &str = "│   ├── ";

    /// Create an empty VFS.
    #[must_use]
    pub fn new() -> Self {
        Self {
            file_map: AHashMap::new(),
            providers: AHashMap::new(),
            sources: Vec::new(),
            layer_index: std::sync::OnceLock::new(),
        }
    }

    /// Returns an iterator over all `(normalized_key, file)` pairs in the VFS.
    pub fn iter(&self) -> impl Iterator<Item = (&NormalizedPath, &VfsFile)> {
        self.file_map.iter()
    }

    /// Returns the canonical provider-chain index owned by this VFS.
    #[must_use]
    pub fn layer_index(&self) -> &LayerIndex {
        self.layer_index.get_or_init(|| self.build_layer_index())
    }

    /// Returns whether this VFS owns a precomputed provider-occurrence [`LayerIndex`].
    ///
    /// Plain constructors intentionally leave this false so engine-style fast paths can keep only the
    /// winner map, provider stacks, and source list. Analysis constructors set it when callers ask for
    /// layer/provenance reports. A VFS constructor that always builds analysis state is not a VFS
    /// constructor; it is a surprise tax with better branding.
    #[must_use]
    pub fn has_layer_index(&self) -> bool {
        self.layer_index.get().is_some()
    }

    pub(crate) fn push_source(&mut self, source: SourceMeta) -> usize {
        let source_index = self.sources.len();
        self.sources.push(source);
        source_index
    }

    pub(crate) fn provider_original_path(
        source: &SourceMeta,
        key: &NormalizedPath,
        file: &VfsFile,
    ) -> PathBuf {
        if source.kind == SourceKind::LooseDir {
            file.path()
                .strip_prefix(&source.path)
                .map_or_else(|_| key_to_path_buf_lossy(key), Path::to_path_buf)
        } else {
            file.path().to_path_buf()
        }
    }

    pub(crate) fn refresh_winner(&mut self, key: &NormalizedPath) {
        if let Some(entry) = self
            .providers
            .get(key)
            .and_then(|providers| providers.last())
        {
            self.file_map
                .insert(key.clone(), entry.provider.file.clone());
        } else {
            self.file_map.remove(key);
            self.providers.remove(key);
        }
    }

    pub(crate) fn winner_source_index(&self, key: &NormalizedPath) -> Option<usize> {
        self.providers
            .get(key)
            .and_then(|providers| providers.last())
            .map(|entry| entry.source_index)
    }

    pub(crate) fn provider_file_for_key_index(
        &self,
        key: &NormalizedPath,
        provider_index: usize,
    ) -> Option<&VfsFile> {
        self.providers
            .get(key)?
            .get(provider_index)
            .map(|entry| &entry.provider.file)
    }

    pub(crate) fn winner_provider_index(&self, key: &NormalizedPath) -> Option<usize> {
        self.providers.get(key)?.len().checked_sub(1)
    }

    pub(crate) fn build_layer_index(&self) -> LayerIndex {
        let mut used = vec![false; self.sources.len()];
        for providers in self.providers.values() {
            for entry in providers {
                used[entry.source_index] = true;
            }
        }

        let mut remap = vec![usize::MAX; self.sources.len()];
        let mut compacted_sources = Vec::new();
        for (old_index, source) in self.sources.iter().cloned().enumerate() {
            if used[old_index] {
                remap[old_index] = compacted_sources.len();
                compacted_sources.push(source);
            }
        }

        let mut files_by_source = vec![Vec::<PathBuf>::new(); compacted_sources.len()];
        for (key, providers) in &self.providers {
            for entry in providers {
                let source = &entry.provider.source;
                let path_for_layer = if source.kind == SourceKind::Archive {
                    key_to_path_buf_bytes(key).unwrap_or_else(|| key_to_path_buf_lossy(key))
                } else {
                    Self::provider_original_path(source, key, &entry.provider.file)
                };
                files_by_source[remap[entry.source_index]].push(path_for_layer);
            }
        }

        let rows = compacted_sources
            .into_iter()
            .zip(files_by_source)
            .collect::<Vec<_>>();
        LayerIndex::from_file_lists(rows)
    }

    pub(crate) fn rebuild_layer_index(&mut self) {
        if self.layer_index.get().is_none() {
            return;
        }
        let layer_index = self.build_layer_index();
        let _ = self.layer_index.take();
        let _ = self.layer_index.set(layer_index);
    }

    /// Returns a parallel iterator over all `(normalized_key, file)` pairs in the VFS.
    #[must_use]
    pub fn par_iter(&self) -> impl ParallelIterator<Item = (&NormalizedPath, &VfsFile)> {
        self.file_map.par_iter()
    }
}

impl Default for VFS {
    fn default() -> Self {
        Self::new()
    }
}
