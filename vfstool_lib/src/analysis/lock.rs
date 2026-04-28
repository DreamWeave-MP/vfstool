// SPDX-License-Identifier: GPL-3.0-only
use super::{LayerIndex, VFS_LOCK_SCHEMA_VERSION, VfsLock, VfsLockEntry};
use crate::{NormalizedPath, VFS, paths::key_to_path_buf_lossy, semantic::ArchiveHashMode};
use rayon::prelude::*;
use std::io;

use super::provider_io::ProviderIoCache;

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
            schema_version: VFS_LOCK_SCHEMA_VERSION,
            entries,
        })
    }

    fn lock_entry_for_key(
        &self,
        vfs: &VFS,
        key: &NormalizedPath,
    ) -> io::Result<Option<VfsLockEntry>> {
        let providers = self.provider_chain(&key_to_path_buf_lossy(key));
        if providers.is_empty() {
            return Ok(None);
        }

        let Some(winner_provider) = providers.last() else {
            return Ok(None);
        };
        if vfs.winner_provider_index(key) != Some(winner_provider.provider_index) {
            return Ok(None);
        }
        let winner_source = &winner_provider.source;
        let mut hash_cache = ProviderIoCache::new();
        let winner_fp = self.fingerprint_for_provider(
            vfs,
            winner_provider,
            &mut hash_cache,
            ArchiveHashMode::WinnerOnly,
        )?;

        Ok(Some(VfsLockEntry {
            key: key_to_path_buf_lossy(key),
            winner_source: winner_source.path.clone(),
            winner_kind: winner_source.kind,
            winner_hash_blake3: winner_fp.as_ref().map(|f| f.to_digest().hex),
            winner_size: winner_fp.as_ref().map(|f| f.to_digest().size),
            provider_count: providers.len(),
        }))
    }
}
