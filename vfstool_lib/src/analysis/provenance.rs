// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{ArchiveHashMode, LayerIndex, ProvenanceChain, ProviderRecord, SourceKind};
use crate::{VFS, normalize_path_in_place};
use ahash::AHashMap;
use std::{
    io,
    path::{Path, PathBuf},
};

use super::provider_io::ContentFingerprint;

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
        let mut key = path.to_path_buf();
        normalize_path_in_place(&mut key);

        let provider_indices = self.sources_containing(&key);
        if provider_indices.is_empty() {
            return Ok(None);
        }

        let Some(winner_idx) = self.current_winner_source_idx(vfs, &key, provider_indices) else {
            return Ok(None);
        };
        let winner = self.sources[winner_idx].clone();
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> =
            AHashMap::new();

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
                SourceKind::Archive => format!("{}::{}", src.path.display(), key.display()),
            };

            providers.push(ProviderRecord {
                source: src,
                resolved_path,
                hash_blake3,
                size,
            });
        }

        Ok(Some(ProvenanceChain {
            key,
            providers,
            winner,
        }))
    }
}
