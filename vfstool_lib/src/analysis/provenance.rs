// SPDX-License-Identifier: GPL-3.0-only
use super::{LayerIndex, ProvenanceChain, ProviderRecord, SourceKind};
use crate::{NormalizedPath, VFS, paths::key_to_path_buf_lossy, semantic::ArchiveHashMode};
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

        let provider_chain = self.provider_chain(path);
        if provider_chain.is_empty() {
            return Ok(None);
        }

        let Some(winner_provider) = provider_chain.last() else {
            return Ok(None);
        };
        if vfs.winner_provider_index(&key) != Some(winner_provider.provider_index) {
            return Ok(None);
        }
        let winner = winner_provider.source.clone();
        let mut hash_cache = ProviderIoCache::new();

        let mut providers = Vec::with_capacity(provider_chain.len());
        for provider in &provider_chain {
            let src = provider.source.clone();
            let (hash_blake3, size) = if with_hashes {
                match self.fingerprint_for_provider(
                    vfs,
                    provider,
                    &mut hash_cache,
                    ArchiveHashMode::AllProviders,
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
                SourceKind::LooseDir => vfs
                    .provider_file_for_key_index(&key, provider.provider_index)
                    .map_or_else(
                        || self.provider_path(provider).display().to_string(),
                        |file| file.path().display().to_string(),
                    ),
                SourceKind::Archive => {
                    format!(
                        "{}::{}",
                        src.path.display(),
                        provider.original_path.display()
                    )
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
