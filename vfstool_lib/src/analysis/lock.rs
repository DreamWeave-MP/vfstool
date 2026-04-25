// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{ArchiveHashMode, LayerIndex, VfsLock, VfsLockEntry};
use crate::VFS;
use ahash::AHashMap;
use rayon::prelude::*;
use std::{
    io,
    path::{Path, PathBuf},
};

use super::provider_io::ContentFingerprint;

impl LayerIndex {
    /// Build deterministic lock manifest from current winners.
    ///
    /// # Errors
    ///
    /// Returns an error when reading winner file content for hashing fails.
    pub fn lock_manifest(&self, vfs: &VFS) -> io::Result<VfsLock> {
        let mut entries: Vec<VfsLockEntry> = self
            .keys()
            .par_iter()
            .map(|key| self.lock_entry_for_key(vfs, key))
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(VfsLock {
            schema_version: 1,
            entries,
        })
    }

    fn lock_entry_for_key(&self, vfs: &VFS, key: &Path) -> io::Result<Option<VfsLockEntry>> {
        let providers = self.sources_containing(key);
        if providers.is_empty() {
            return Ok(None);
        }

        let Some(winner_idx) = self.current_winner_source_idx(vfs, key, providers) else {
            return Ok(None);
        };
        let winner_source = &self.sources[winner_idx];
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> =
            AHashMap::new();
        let winner_fp = self.fingerprint_for_provider(
            vfs,
            winner_idx,
            key,
            &mut hash_cache,
            ArchiveHashMode::WinnerOnly,
        )?;

        Ok(Some(VfsLockEntry {
            key: key.to_path_buf(),
            winner_source: winner_source.path.clone(),
            winner_kind: winner_source.kind,
            winner_hash_blake3: winner_fp.as_ref().map(|f| f.to_digest().hex),
            winner_size: winner_fp.as_ref().map(|f| f.to_digest().size),
            provider_count: providers.len(),
        }))
    }
}
