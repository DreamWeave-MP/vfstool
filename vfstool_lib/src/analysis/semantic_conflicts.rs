// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{
    LayerIndex, SemanticConflict, SemanticConflictReport, SemanticOpts, SemanticProvider,
    SemanticRelation,
};
use crate::{
    VFS,
    semantic::{AssetClass, analyze_pair},
};
use ahash::AHashSet;
use rayon::prelude::*;
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(any(feature = "bsa", feature = "zip"))]
use super::provider_io::SharedArchiveFileCache;
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

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_cache = ProviderIoCache::new_shared_archive_file_cache();

        let mut entries: Vec<SemanticConflict> = keys
            .par_iter()
            .map(|key| {
                #[cfg(any(feature = "bsa", feature = "zip"))]
                {
                    self.semantic_conflict_for_key_with_archive_cache(
                        vfs,
                        key,
                        opts,
                        archive_cache.clone(),
                    )
                }
                #[cfg(not(any(feature = "bsa", feature = "zip")))]
                {
                    self.semantic_conflict_for_key_no_cache(vfs, key, opts)
                }
            })
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));

        Ok(SemanticConflictReport { entries })
    }

    #[cfg(any(test, not(any(feature = "bsa", feature = "zip"))))]
    pub(super) fn semantic_conflict_for_key_no_cache(
        &self,
        vfs: &VFS,
        key: &Path,
        opts: SemanticOpts,
    ) -> io::Result<Option<SemanticConflict>> {
        let mut hash_cache = ProviderIoCache::new();
        self.semantic_conflict_for_key(vfs, key, opts, &mut hash_cache)
    }

    #[cfg(any(feature = "bsa", feature = "zip"))]
    fn semantic_conflict_for_key_with_archive_cache(
        &self,
        vfs: &VFS,
        key: &Path,
        opts: SemanticOpts,
        archive_cache: SharedArchiveFileCache,
    ) -> io::Result<Option<SemanticConflict>> {
        let mut hash_cache = ProviderIoCache::with_shared_archive_file_cache(archive_cache);
        self.semantic_conflict_for_key(vfs, key, opts, &mut hash_cache)
    }

    fn semantic_conflict_for_key(
        &self,
        vfs: &VFS,
        key: &Path,
        opts: SemanticOpts,
        hash_cache: &mut ProviderIoCache,
    ) -> io::Result<Option<SemanticConflict>> {
        let provider_indices = self.sources_containing(key);
        if provider_indices.len() < 2 {
            return Ok(None);
        }

        let Some(winner_idx) = self.current_winner_source_idx(vfs, key, provider_indices) else {
            return Ok(None);
        };

        let winner_source = self.sources[winner_idx].clone();
        let winner_bytes = if opts.include_semantic_deltas {
            self.read_provider_bytes(vfs, winner_idx, key, hash_cache)
        } else {
            Ok(None)
        }?;
        let winner_fp = if let Some(bytes) = &winner_bytes {
            Some(fingerprint_bytes(bytes))
        } else {
            self.fingerprint_for_provider(vfs, winner_idx, key, hash_cache, opts.archive_hash_mode)?
        };

        let mut seen_hashes = AHashSet::<String>::new();
        let mut providers = Vec::with_capacity(provider_indices.len());
        let mut inferred_asset_class = AssetClass::Unknown;

        for &idx in provider_indices {
            let src = self.sources[idx].clone();
            let current_bytes = if opts.include_semantic_deltas {
                self.read_provider_bytes(vfs, idx, key, hash_cache)?
            } else {
                None
            };
            let current = if let Some(bytes) = &current_bytes {
                Some(fingerprint_bytes(bytes))
            } else {
                self.fingerprint_for_provider(vfs, idx, key, hash_cache, opts.archive_hash_mode)?
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
            all_identical: winner_fp.is_some() && seen_hashes.len() == 1,
            distinct_versions: seen_hashes.len(),
        }))
    }
}
