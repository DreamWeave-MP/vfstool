// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
use ahash::AHashMap;
use rayon::prelude::*;

use crate::{LayerIndex, NormalizedPath, VfsFile};

impl VFS {
    pub(super) const DIR_PREFIX: &str = "├── ";
    pub(super) const FILE_PREFIX: &str = "│   ├── ";

    /// Create an empty VFS.
    #[must_use]
    pub fn new() -> Self {
        Self {
            file_map: AHashMap::new(),
            layer_index: LayerIndex::from_file_lists([]),
        }
    }

    /// Returns an iterator over all `(normalized_key, file)` pairs in the VFS.
    pub fn iter(&self) -> impl Iterator<Item = (&NormalizedPath, &VfsFile)> {
        self.file_map.iter()
    }

    /// Returns the canonical provider-chain index owned by this VFS.
    #[must_use]
    pub fn layer_index(&self) -> &LayerIndex {
        &self.layer_index
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
