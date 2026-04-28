// SPDX-License-Identifier: GPL-3.0-only
use super::LayerIndex;
use crate::{
    NormalizedPath, VFS,
    semantic::{
        AssetClass, SemanticConflict, SemanticConflictReport, SemanticOpts, SemanticProvider,
        SemanticRelation, analyze_pair,
    },
};
use ahash::AHashSet;
use rayon::prelude::*;
use std::{
    io,
    path::{Path, PathBuf},
};

use super::provider_io::{ProviderIoCache, fingerprint_bytes};

impl LayerIndex {
    /// Build semantic conflicts for all paths with multiple providers.
    ///
    /// # Errors
    ///
    /// Returns an error when content hashing for any provider fails.
    pub fn semantic_conflicts(&self, vfs: &VFS) -> io::Result<SemanticConflictReport> {
        self.semantic_conflicts_with_opts(vfs, SemanticOpts::default())
    }

    /// Build semantic conflicts for all paths with multiple providers.
    ///
    /// # Errors
    ///
    /// Returns an error when content hashing for any provider fails.
    pub fn semantic_conflicts_with_opts(
        &self,
        vfs: &VFS,
        opts: SemanticOpts,
    ) -> io::Result<SemanticConflictReport> {
        let mut keys: Vec<PathBuf> = self
            .path_to_sources
            .iter()
            .filter_map(|(k, providers)| {
                if providers.len() > 1 {
                    Some(k.clone().into_path_buf())
                } else {
                    None
                }
            })
            .collect();
        keys.sort();

        let mut entries: Vec<SemanticConflict> = keys
            .par_iter()
            .map(|key| self.semantic_conflict_for_key_no_cache(vfs, key, opts))
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));

        Ok(SemanticConflictReport { entries })
    }

    pub(super) fn semantic_conflict_for_key_no_cache(
        &self,
        vfs: &VFS,
        key: &Path,
        opts: SemanticOpts,
    ) -> io::Result<Option<SemanticConflict>> {
        let mut hash_cache = ProviderIoCache::new();
        self.semantic_conflict_for_key(vfs, key, opts, &mut hash_cache)
    }

    fn semantic_conflict_for_key(
        &self,
        vfs: &VFS,
        key: &Path,
        opts: SemanticOpts,
        hash_cache: &mut ProviderIoCache,
    ) -> io::Result<Option<SemanticConflict>> {
        let normalized_key = NormalizedPath::new(key.as_os_str().as_encoded_bytes());
        let provider_chain = self.provider_chain(key);
        if provider_chain.len() < 2 {
            return Ok(None);
        }

        let Some(winner_provider) = provider_chain.last() else {
            return Ok(None);
        };

        if vfs.winner_provider_index(&normalized_key) != Some(winner_provider.provider_index) {
            return Ok(None);
        }

        let winner_source = winner_provider.source.clone();
        let winner_bytes = if opts.include_semantic_deltas {
            self.read_provider_bytes(vfs, winner_provider, hash_cache, opts.archive_hash_mode)
        } else {
            Ok(None)
        }?;
        let winner_fp = if let Some(bytes) = &winner_bytes {
            Some(fingerprint_bytes(bytes))
        } else {
            self.fingerprint_for_provider(vfs, winner_provider, hash_cache, opts.archive_hash_mode)?
        };

        let mut seen_hashes = AHashSet::<String>::new();
        let mut providers = Vec::with_capacity(provider_chain.len());
        let mut inferred_asset_class = AssetClass::Unknown;
        let mut hashed_provider_count = 0usize;

        for provider in &provider_chain {
            let src = provider.source.clone();
            let current_bytes = if opts.include_semantic_deltas {
                self.read_provider_bytes(vfs, provider, hash_cache, opts.archive_hash_mode)?
            } else {
                None
            };
            let current = if let Some(bytes) = &current_bytes {
                Some(fingerprint_bytes(bytes))
            } else {
                self.fingerprint_for_provider(vfs, provider, hash_cache, opts.archive_hash_mode)?
            };

            let semantic_delta_to_winner = if opts.include_semantic_deltas {
                match (&winner_bytes, &current_bytes) {
                    (Some(winner), Some(current)) => {
                        let (asset_class, delta) = analyze_pair(key, current, winner);
                        inferred_asset_class = asset_class;
                        Some(delta)
                    }
                    _ => None,
                }
            } else {
                None
            };

            let (relation, hash_blake3, size) = match (&winner_fp, &current) {
                (Some(w), Some(c)) => {
                    hashed_provider_count += 1;
                    let rel = if w.digest == c.digest {
                        SemanticRelation::IdenticalToWinner
                    } else {
                        SemanticRelation::DifferentFromWinner
                    };
                    let digest = c.to_digest();
                    seen_hashes.insert(digest.hex.clone());
                    (rel, Some(digest.hex), Some(digest.size))
                }
                (_, Some(c)) => {
                    hashed_provider_count += 1;
                    let digest = c.to_digest();
                    seen_hashes.insert(digest.hex.clone());
                    (
                        SemanticRelation::Unknown,
                        Some(digest.hex),
                        Some(digest.size),
                    )
                }
                (_, None) => (SemanticRelation::Unknown, None, None),
            };

            providers.push(SemanticProvider {
                source: src,
                relation,
                hash_blake3,
                size,
                semantic_delta_to_winner,
            });
        }

        Ok(Some(SemanticConflict {
            key: key.to_path_buf(),
            winner: winner_source,
            providers,
            asset_class: inferred_asset_class,
            all_identical: winner_fp.is_some()
                && hashed_provider_count == provider_chain.len()
                && seen_hashes.len() == 1,
            distinct_versions: seen_hashes.len(),
        }))
    }
}
