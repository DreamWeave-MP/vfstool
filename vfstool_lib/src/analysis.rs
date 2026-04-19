// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{VFS, normalize_path_in_place};
use ahash::{AHashMap, AHashSet};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Source type in the load order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum SourceKind {
    /// A loose data directory.
    LooseDir,
    /// An archive source (BSA/BA2/ZIP/PK3).
    Archive,
}

/// A source entry in load-order position.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SourceMeta {
    /// Absolute path to the source.
    pub path: PathBuf,
    /// Source type.
    pub kind: SourceKind,
}

/// Canonical provider index for all normalized VFS keys.
///
/// `path_to_sources[key]` is ordered low -> high priority.
pub struct LayerIndex {
    /// Sources in load-order position.
    pub sources: Vec<SourceMeta>,
    path_to_sources: AHashMap<PathBuf, Vec<usize>>,
}

/// One provider in a per-key provenance chain.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ProviderRecord {
    /// Source metadata.
    pub source: SourceMeta,
    /// Absolute loose path or archive-entry display path.
    pub resolved_path: String,
    /// Optional content hash (unavailable for some archive providers).
    pub hash_blake3: Option<String>,
    /// Optional byte size.
    pub size: Option<u64>,
}

/// Full load-order chain for a key.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ProvenanceChain {
    /// Normalized key queried.
    pub key: PathBuf,
    /// Providers in low -> high priority order.
    pub providers: Vec<ProviderRecord>,
    /// Winning source.
    pub winner: SourceMeta,
}

/// Per-provider relation to winner content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum SemanticRelation {
    /// Byte-identical to winner.
    IdenticalToWinner,
    /// Different bytes from winner.
    DifferentFromWinner,
    /// Content unavailable.
    Unknown,
}

/// Semantic info for one provider.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SemanticProvider {
    /// Source metadata.
    pub source: SourceMeta,
    /// Relation to winning content.
    pub relation: SemanticRelation,
    /// Optional content hash.
    pub hash_blake3: Option<String>,
    /// Optional size.
    pub size: Option<u64>,
}

/// Semantic conflict for one key with multiple providers.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SemanticConflict {
    /// Normalized key.
    pub key: PathBuf,
    /// Winning source.
    pub winner: SourceMeta,
    /// Providers in low -> high priority order.
    pub providers: Vec<SemanticProvider>,
    /// True if every available hash equals the winner hash.
    pub all_identical: bool,
    /// Count of unique available content hashes.
    pub distinct_versions: usize,
}

/// Semantic conflicts across the load order.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SemanticConflictReport {
    /// One entry per conflicting key.
    pub entries: Vec<SemanticConflict>,
}

/// Deterministic lock file output.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct VfsLock {
    /// Schema version.
    pub schema_version: u32,
    /// Deterministically sorted lock entries.
    pub entries: Vec<VfsLockEntry>,
}

/// One deterministic lock entry.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct VfsLockEntry {
    /// Normalized key.
    pub key: PathBuf,
    /// Winning source.
    pub winner_source: PathBuf,
    /// Winner source kind.
    pub winner_kind: SourceKind,
    /// Winner hash (hex) when available.
    pub winner_hash_blake3: Option<String>,
    /// Winner size when available.
    pub winner_size: Option<u64>,
    /// Number of providers.
    pub provider_count: usize,
}

/// Reorder operation for what-if simulations.
pub enum ReorderOp {
    /// Swap two sources by exact path.
    Swap(PathBuf, PathBuf),
}

/// Per-source win delta in a simulation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SourceDelta {
    /// Source path.
    pub source: PathBuf,
    /// Wins before simulation.
    pub wins_before: usize,
    /// Wins after simulation.
    pub wins_after: usize,
}

/// Summary of a what-if simulation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SimulationDelta {
    /// Number of keys with different winners.
    pub changed_winners: usize,
    /// Number of keys with unchanged winners.
    pub unchanged_winners: usize,
    /// Per-source win deltas.
    pub by_source_gain_loss: Vec<SourceDelta>,
    /// Small sorted sample of changed keys.
    pub changed_keys_sample: Vec<PathBuf>,
}

#[derive(Clone)]
struct ContentFingerprint {
    digest: [u8; 32],
    size: u64,
}

impl ContentFingerprint {
    fn digest_hex(&self) -> String {
        self.digest.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl LayerIndex {
    /// Build a provider index from ordered `(source_meta, normalized_paths)` pairs.
    pub fn from_file_lists(
        sources: impl IntoIterator<Item = (SourceMeta, Vec<PathBuf>)>,
    ) -> Self {
        let mut source_paths = Vec::new();
        let mut path_to_sources: AHashMap<PathBuf, Vec<usize>> = AHashMap::new();

        for (source_meta, files) in sources {
            let idx = source_paths.len();
            source_paths.push(source_meta);

            for mut path in files {
                normalize_path_in_place(&mut path);
                path_to_sources.entry(path).or_default().push(idx);
            }
        }

        Self { sources: source_paths, path_to_sources }
    }

    /// Returns all normalized keys in sorted order.
    pub fn keys(&self) -> Vec<PathBuf> {
        let mut keys: Vec<PathBuf> = self.path_to_sources.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Returns source indices that provide `path`, in load order.
    pub fn sources_containing(&self, path: &Path) -> &[usize] {
        let mut normalized = path.to_path_buf();
        normalize_path_in_place(&mut normalized);
        self.path_to_sources
            .get(&normalized)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Return the full source chain for one key.
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

        let winner_idx = *provider_indices.last().expect("providers are non-empty");
        let winner = self.sources[winner_idx].clone();
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> = AHashMap::new();

        let mut providers = Vec::with_capacity(provider_indices.len());
        for &idx in provider_indices {
            let src = self.sources[idx].clone();
            let (hash_blake3, size) = if with_hashes {
                match self.fingerprint_for_provider(vfs, idx, &key, &mut hash_cache)? {
                    Some(fp) => (Some(fp.digest_hex()), Some(fp.size)),
                    None => (None, None),
                }
            } else {
                (None, None)
            };

            let resolved_path = match src.kind {
                SourceKind::LooseDir => src.path.join(&key).display().to_string(),
                SourceKind::Archive => format!("{}::{}", src.path.display(), key.display()),
            };

            providers.push(ProviderRecord { source: src, resolved_path, hash_blake3, size });
        }

        Ok(Some(ProvenanceChain { key, providers, winner }))
    }

    /// Build semantic conflicts for all paths with multiple providers.
    pub fn semantic_conflicts(&self, vfs: &VFS) -> io::Result<SemanticConflictReport> {
        let mut entries = Vec::new();
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> = AHashMap::new();

        let mut keys: Vec<PathBuf> = self
            .path_to_sources
            .iter()
            .filter_map(|(k, providers)| {
                if providers.len() > 1 {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        keys.sort();

        for key in keys {
            let provider_indices = self.sources_containing(&key);
            if provider_indices.is_empty() {
                continue;
            }

            let winner_idx = *provider_indices.last().expect("providers are non-empty");
            let winner_source = self.sources[winner_idx].clone();
            let winner_fp = self.fingerprint_for_provider(vfs, winner_idx, &key, &mut hash_cache)?;

            let mut seen_hashes = AHashSet::<String>::new();
            let mut providers = Vec::with_capacity(provider_indices.len());

            for &idx in provider_indices {
                let src = self.sources[idx].clone();
                let current = self.fingerprint_for_provider(vfs, idx, &key, &mut hash_cache)?;

                let (relation, hash_blake3, size) = match (&winner_fp, &current) {
                    (Some(w), Some(c)) => {
                        let rel = if w.digest == c.digest {
                            SemanticRelation::IdenticalToWinner
                        } else {
                            SemanticRelation::DifferentFromWinner
                        };
                        let hex = c.digest_hex();
                        seen_hashes.insert(hex.clone());
                        (rel, Some(hex), Some(c.size))
                    }
                    (_, Some(c)) => {
                        let hex = c.digest_hex();
                        seen_hashes.insert(hex.clone());
                        (SemanticRelation::Unknown, Some(hex), Some(c.size))
                    }
                    (_, None) => (SemanticRelation::Unknown, None, None),
                };

                providers.push(SemanticProvider {
                    source: src,
                    relation,
                    hash_blake3,
                    size,
                });
            }

            let all_identical = !seen_hashes.is_empty() && seen_hashes.len() == 1;
            let distinct_versions = seen_hashes.len();

            entries.push(SemanticConflict {
                key,
                winner: winner_source,
                providers,
                all_identical,
                distinct_versions,
            });
        }

        Ok(SemanticConflictReport { entries })
    }

    /// Build deterministic lock manifest from current winners.
    pub fn lock_manifest(&self, vfs: &VFS) -> io::Result<VfsLock> {
        let mut entries = Vec::new();
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> = AHashMap::new();

        for key in self.keys() {
            let providers = self.sources_containing(&key);
            if providers.is_empty() {
                continue;
            }
            let winner_idx = *providers.last().expect("providers are non-empty");
            let winner_source = &self.sources[winner_idx];
            let winner_fp = self.fingerprint_for_provider(vfs, winner_idx, &key, &mut hash_cache)?;

            entries.push(VfsLockEntry {
                key,
                winner_source: winner_source.path.clone(),
                winner_kind: winner_source.kind,
                winner_hash_blake3: winner_fp.as_ref().map(ContentFingerprint::digest_hex),
                winner_size: winner_fp.as_ref().map(|f| f.size),
                provider_count: providers.len(),
            });
        }

        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(VfsLock { schema_version: 1, entries })
    }

    /// Simulate a simple load-order edit and report winner deltas.
    pub fn simulate(&self, vfs: &VFS, op: ReorderOp) -> io::Result<SimulationDelta> {
        let mut order: Vec<usize> = (0..self.sources.len()).collect();
        match op {
            ReorderOp::Swap(a, b) => {
                let ai = self
                    .sources
                    .iter()
                    .position(|s| s.path == a)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "swap source A not found"))?;
                let bi = self
                    .sources
                    .iter()
                    .position(|s| s.path == b)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "swap source B not found"))?;
                order.swap(ai, bi);
            }
        }

        let mut rank_by_source = vec![0usize; order.len()];
        for (rank, src_idx) in order.iter().enumerate() {
            rank_by_source[*src_idx] = rank;
        }

        let mut wins_before = vec![0usize; self.sources.len()];
        let mut wins_after = vec![0usize; self.sources.len()];
        let mut changed = Vec::<PathBuf>::new();

        for key in self.keys() {
            let providers = self.sources_containing(&key);
            if providers.is_empty() {
                continue;
            }

            let before_idx = self.current_winner_source_idx(vfs, &key, providers);
            let after_idx = providers
                .iter()
                .copied()
                .max_by_key(|idx| rank_by_source[*idx])
                .expect("providers are non-empty");

            if let Some(idx) = before_idx {
                wins_before[idx] += 1;
            }
            wins_after[after_idx] += 1;

            if Some(after_idx) != before_idx {
                changed.push(key);
            }
        }

        changed.sort();
        let mut rows = Vec::with_capacity(self.sources.len());
        for idx in 0..self.sources.len() {
            rows.push(SourceDelta {
                source: self.sources[idx].path.clone(),
                wins_before: wins_before[idx],
                wins_after: wins_after[idx],
            });
        }

        Ok(SimulationDelta {
            changed_winners: changed.len(),
            unchanged_winners: wins_after.iter().sum::<usize>() - changed.len(),
            by_source_gain_loss: rows,
            changed_keys_sample: changed.into_iter().take(100).collect(),
        })
    }

    fn current_winner_source_idx(&self, vfs: &VFS, key: &Path, providers: &[usize]) -> Option<usize> {
        let winner = vfs.get_file(key)?;
        if winner.is_loose() {
            providers.iter().copied().find(|idx| {
                self.sources[*idx].kind == SourceKind::LooseDir
                    && winner.path().starts_with(&self.sources[*idx].path)
            })
        } else {
            let parent = winner.parent_archive_path()?;
            providers.iter().copied().find(|idx| {
                self.sources[*idx].kind == SourceKind::Archive
                    && self.sources[*idx].path.to_string_lossy() == parent
            })
        }
    }

    fn fingerprint_for_provider(
        &self,
        vfs: &VFS,
        source_idx: usize,
        key: &Path,
        cache: &mut AHashMap<(usize, PathBuf), Option<ContentFingerprint>>,
    ) -> io::Result<Option<ContentFingerprint>> {
        let cache_key = (source_idx, key.to_path_buf());
        if let Some(hit) = cache.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[source_idx];
        let fp = match src.kind {
            SourceKind::LooseDir => {
                let path = src.path.join(key);
                if !path.exists() {
                    None
                } else {
                    Some(hash_reader(std::fs::File::open(path)?)?)
                }
            }
            SourceKind::Archive => {
                // Archive content-level hashing is intentionally conservative here.
                // The source chain is still exposed; hash may be unavailable.
                let _ = vfs;
                None
            }
        };

        cache.insert(cache_key, fp.clone());
        Ok(fp)
    }
}

fn hash_reader(mut reader: impl Read) -> io::Result<ContentFingerprint> {
    let mut hasher = blake3::Hasher::new();
    let mut size = 0u64;
    let mut buf = [0u8; 65536];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }

    Ok(ContentFingerprint {
        digest: *hasher.finalize().as_bytes(),
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(name);
            fs::create_dir_all(&dir).expect("failed to create temp dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, rel: &str, data: &[u8]) {
            let target = self.0.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("failed to create parent dir");
            }
            fs::write(target, data).expect("failed to write test file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn provenance_chain_respects_load_order() {
        let low = TempDir::new("analysis_provenance_low");
        let high = TempDir::new("analysis_provenance_high");
        low.write("textures/a.dds", b"low");
        high.write("textures/a.dds", b"high");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let chain = index
            .provenance(&vfs, Path::new("textures/a.dds"), false)
            .expect("provenance should succeed")
            .expect("path should exist");

        assert_eq!(chain.providers.len(), 2);
        assert_eq!(chain.providers[0].source.path, low.path());
        assert_eq!(chain.providers[1].source.path, high.path());
        assert_eq!(chain.winner.path, high.path());
    }

    #[test]
    fn semantic_conflicts_identical_and_different() {
        let low = TempDir::new("analysis_semantic_low");
        let high = TempDir::new("analysis_semantic_high");

        low.write("textures/same.dds", b"identical");
        high.write("textures/same.dds", b"identical");
        low.write("textures/diff.dds", b"aaa");
        high.write("textures/diff.dds", b"bbb");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let report = index
            .semantic_conflicts(&vfs)
            .expect("semantic conflict report should succeed");

        let same = report
            .entries
            .iter()
            .find(|e| e.key == Path::new("textures/same.dds"))
            .expect("expected same.dds entry");
        assert!(same.all_identical);
        assert_eq!(same.distinct_versions, 1);

        let diff = report
            .entries
            .iter()
            .find(|e| e.key == Path::new("textures/diff.dds"))
            .expect("expected diff.dds entry");
        assert!(!diff.all_identical);
        assert_eq!(diff.distinct_versions, 2);
    }

    #[test]
    fn lock_manifest_is_sorted() {
        let data = TempDir::new("analysis_lock_sorted");
        data.write("textures/z.dds", b"z");
        data.write("textures/a.dds", b"a");

        let (vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);
        let lock = index.lock_manifest(&vfs).expect("lock manifest should succeed");
        assert_eq!(lock.schema_version, 1);
        assert_eq!(lock.entries[0].key, PathBuf::from("textures/a.dds"));
        assert_eq!(lock.entries[1].key, PathBuf::from("textures/z.dds"));
    }

    #[test]
    fn simulate_swap_changes_winner() {
        let low = TempDir::new("analysis_sim_low");
        let high = TempDir::new("analysis_sim_high");
        low.write("textures/a.dds", b"low");
        high.write("textures/a.dds", b"high");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let delta = index
            .simulate(
                &vfs,
                ReorderOp::Swap(low.path().to_path_buf(), high.path().to_path_buf()),
            )
            .expect("simulate should succeed");

        assert_eq!(delta.changed_winners, 1);
    }
}
