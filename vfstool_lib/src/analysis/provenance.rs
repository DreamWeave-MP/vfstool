// SPDX-License-Identifier: GPL-3.0-only
use super::{LayerIndex, ProvenanceChain, ProviderRecord, SourceKind};
use crate::{
    NormalizedPath, VFS,
    paths::{key_to_path_buf_lossy, key_to_string_lossy},
    semantic::ArchiveHashMode,
};
use std::{io, path::Path};

use super::provider_io::ProviderIoCache;

impl LayerIndex {
    /// Return the full source chain for one key.
    ///
    /// # Errors
    ///
    /// Returns an error when content hashing is requested and file reads fail.
    pub fn provenance(
        &self,
        vfs: &VFS,
        path: &Path,
        with_hashes: bool,
    ) -> io::Result<Option<ProvenanceChain>> {
        let key = NormalizedPath::new(path.as_os_str().as_encoded_bytes());

        let provider_indices = self.sources_containing(&key);
        if provider_indices.is_empty() {
            return Ok(None);
        }

        let Some(winner_idx) = Self::current_winner_source_idx(vfs, &key, provider_indices) else {
            return Ok(None);
        };
        let winner = self.sources[winner_idx].clone();
        let mut hash_cache = ProviderIoCache::new();

        let mut providers = Vec::with_capacity(provider_indices.len());
        for &idx in provider_indices {
            let src = self.sources[idx].clone();
            let (hash_blake3, size) = if with_hashes {
                match self.fingerprint_for_provider(
                    vfs,
                    idx,
                    &key,
                    &mut hash_cache,
                    ArchiveHashMode::WinnerOnly,
                )? {
                    Some(fp) => {
                        let digest = fp.to_digest();
                        (Some(digest.hex), Some(digest.size))
                    }
                    None => (None, None),
                }
            } else {
                (None, None)
            };

            let resolved_path = match src.kind {
                SourceKind::LooseDir => self.provider_path(idx, &key).display().to_string(),
                SourceKind::Archive => {
                    format!("{}::{}", src.path.display(), key_to_string_lossy(&key))
                }
            };

            providers.push(ProviderRecord {
                source: src,
                resolved_path,
                hash_blake3,
                size,
            });
        }

        Ok(Some(ProvenanceChain {
            key: key_to_path_buf_lossy(&key),
            providers,
            winner,
        }))
    }
}
