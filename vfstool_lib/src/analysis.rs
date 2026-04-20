// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{
    AssetClass, ContentDigest, NormalizedKey, SemanticDelta, SourceId, VFS, analyze_pair,
    normalize_path_in_place, path_glob_matches,
};
use ahash::{AHashMap, AHashSet};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Source type in the load order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum SourceKind {
    /// A loose data directory.
    LooseDir,
    /// An archive source (BSA/BA2/ZIP/PK3).
    Archive,
}

/// A source entry in load-order position.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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
    path_to_sources: AHashMap<NormalizedKey, Vec<usize>>,
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
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticProvider {
    /// Source metadata.
    pub source: SourceMeta,
    /// Relation to winning content.
    pub relation: SemanticRelation,
    /// Optional content hash.
    pub hash_blake3: Option<String>,
    /// Optional size.
    pub size: Option<u64>,
    /// Optional semantic delta compared to winner content.
    pub semantic_delta_to_winner: Option<SemanticDelta>,
}

/// Semantic conflict for one key with multiple providers.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticConflict {
    /// Normalized key.
    pub key: PathBuf,
    /// Winning source.
    pub winner: SourceMeta,
    /// Providers in low -> high priority order.
    pub providers: Vec<SemanticProvider>,
    /// Inferred asset class.
    pub asset_class: AssetClass,
    /// True if every available hash equals the winner hash.
    pub all_identical: bool,
    /// Count of unique available content hashes.
    pub distinct_versions: usize,
}

/// Semantic conflicts across the load order.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticConflictReport {
    /// One entry per conflicting key.
    pub entries: Vec<SemanticConflict>,
}

/// Archive hashing mode for semantic conflict analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum ArchiveHashMode {
    /// Do not hash archive providers.
    Disabled,
    /// Hash only archive providers that currently win in the VFS.
    WinnerOnly,
    /// Hash all archive providers when available.
    ///
    /// Current implementation hashes winners and gracefully falls back to
    /// unknown for non-winning archive providers.
    AllProviders,
}

/// Semantic conflict report options.
#[derive(Debug, Clone, Copy)]
pub struct SemanticOpts {
    /// Archive hashing behavior.
    pub archive_hash_mode: ArchiveHashMode,
    /// Include semantic analyzer deltas where possible.
    pub include_semantic_deltas: bool,
}

impl Default for SemanticOpts {
    fn default() -> Self {
        Self {
            archive_hash_mode: ArchiveHashMode::WinnerOnly,
            include_semantic_deltas: false,
        }
    }
}

/// Deterministic lock file output.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct VfsLock {
    /// Schema version.
    pub schema_version: u32,
    /// Deterministically sorted lock entries.
    pub entries: Vec<VfsLockEntry>,
}

/// One deterministic lock entry.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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
#[derive(Debug, Clone)]
pub enum ReorderOp {
    /// Swap two sources by exact path.
    Swap(PathBuf, PathBuf),
    /// Move one source before another source.
    MoveBefore {
        /// Source to move.
        source: PathBuf,
        /// Destination source before which `source` is inserted.
        before: PathBuf,
    },
    /// Move one source after another source.
    MoveAfter {
        /// Source to move.
        source: PathBuf,
        /// Destination source after which `source` is inserted.
        after: PathBuf,
    },
    /// Set the full explicit load order.
    FullOrder(Vec<PathBuf>),
}

/// Simulation options.
#[derive(Debug, Clone)]
pub struct SimOpts {
    /// Maximum number of changed keys included in sample output.
    pub sample_limit: usize,
    /// Optional impact bucket globs (e.g. `textures/**`, `meshes/**`).
    pub impact_buckets: Vec<String>,
}

/// Condition under which an impact rule applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serialize", serde(rename_all = "snake_case"))]
pub enum HeuristicCondition {
    /// Applies whenever the winner changed for the key.
    WinnerChanged,
    /// Applies only when winner changed and semantic analysis marks behavior change.
    WinnerChangedAndSemanticBehaviorChanging,
}

/// One weighted impact heuristic.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ImpactHeuristic {
    /// Rule name used in diagnostics.
    pub name: String,
    /// Path glob for rule scope.
    pub path_glob: String,
    /// Rule weight added to total score when matched.
    pub weight: f32,
    /// Match condition for this rule.
    pub condition: HeuristicCondition,
}

/// Impact scoring profile.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ImpactProfile {
    /// Ordered list of weighted heuristics.
    pub heuristics: Vec<ImpactHeuristic>,
}

/// One risky changed key row.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct RiskyChange {
    /// Changed key.
    pub key: PathBuf,
    /// Accumulated impact score.
    pub score: f32,
    /// Names of matched heuristic rules.
    pub reasons: Vec<String>,
}

/// Impact score aggregate per bucket glob.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct BucketImpact {
    /// Bucket glob.
    pub bucket: String,
    /// Summed score for changed keys in this bucket.
    pub score: f32,
}

/// Impact scoring result.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ImpactReport {
    /// Total accumulated score across changed keys.
    pub overall_score: f32,
    /// Coarse risk level derived from total score.
    pub risk_level: RiskLevel,
    /// Per-bucket impact summaries.
    pub by_bucket: Vec<BucketImpact>,
    /// Top changed keys ranked by impact score.
    pub top_risky_changes: Vec<RiskyChange>,
}

impl Default for SimOpts {
    fn default() -> Self {
        Self {
            sample_limit: 100,
            impact_buckets: Vec::new(),
        }
    }
}

/// Change count for one impact bucket.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct BucketDelta {
    /// Bucket glob.
    pub bucket: String,
    /// Count of changed winners that matched this bucket.
    pub changed_winners: usize,
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
    /// Change totals by optional bucket globs.
    pub by_bucket: Vec<BucketDelta>,
    /// Small sorted sample of changed keys.
    pub changed_keys_sample: Vec<PathBuf>,
}

/// Per-key drift kind when comparing current state to a lock file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum DriftKind {
    /// Key exists now but not in lock.
    Added,
    /// Key exists in lock but not now.
    Removed,
    /// Winner source path changed.
    WinnerSourceChanged,
    /// Winner content hash changed.
    WinnerHashChanged,
    /// Provider count changed.
    ProviderCountChanged,
}

/// One drift report row.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct DriftEntry {
    /// Key whose lock relation drifted.
    pub key: PathBuf,
    /// Drift category.
    pub kind: DriftKind,
}

/// Drift report against a lock manifest.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct DriftReport {
    /// Per-key drift entries.
    pub entries: Vec<DriftEntry>,
    /// Aggregated counts by drift kind.
    pub counts: BTreeMap<DriftKind, usize>,
}

/// Optional risk level for candidate planning workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum RiskLevel {
    /// Lowest risk.
    Low,
    /// Medium risk.
    Medium,
    /// High risk.
    High,
    /// Highest risk.
    Critical,
}

/// Candidate planning options.
#[derive(Debug, Clone, Copy)]
pub struct CandidatePlanOpts {
    /// Include semantic equality checks for conflicting files.
    pub include_semantic: bool,
}

impl Default for CandidatePlanOpts {
    fn default() -> Self {
        Self {
            include_semantic: true,
        }
    }
}

/// One conflict row in a candidate preflight plan.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct CandidateConflict {
    /// Normalized key.
    pub key: PathBuf,
    /// Current winner source path.
    pub current_winner_source: PathBuf,
    /// Candidate file path.
    pub candidate_file: PathBuf,
    /// Whether candidate content differs from current winner.
    pub semantic_differs: Option<bool>,
    /// Optional risk level placeholder.
    pub risk: Option<RiskLevel>,
}

/// Candidate planner summary metrics.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct CandidatePlanSummary {
    /// Count of net-new files.
    pub additions: usize,
    /// Count of path conflicts.
    pub conflicts: usize,
    /// Count of keys whose winner would be displaced.
    pub displaced_winners: usize,
}

/// Candidate preflight plan.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct CandidatePlan {
    /// Normalized keys that would be newly added.
    pub additions: Vec<PathBuf>,
    /// Conflicting keys and metadata.
    pub conflicts: Vec<CandidateConflict>,
    /// Keys whose current winners would be replaced by candidate content.
    pub displaced_winners: Vec<PathBuf>,
    /// Summary counters.
    pub summary: CandidatePlanSummary,
}

#[derive(Clone)]
struct ContentFingerprint {
    digest: [u8; 32],
    size: u64,
}

impl ContentFingerprint {
    fn to_digest(&self) -> ContentDigest {
        ContentDigest::blake3(self.digest, self.size)
    }
}

impl LayerIndex {
    /// Build a provider index from ordered `(source_meta, normalized_paths)` pairs.
    pub fn from_file_lists(sources: impl IntoIterator<Item = (SourceMeta, Vec<PathBuf>)>) -> Self {
        let mut source_paths = Vec::new();
        let mut path_to_sources: AHashMap<NormalizedKey, Vec<usize>> = AHashMap::new();

        for (source_meta, files) in sources {
            let idx = source_paths.len();
            source_paths.push(source_meta);

            for path in files {
                path_to_sources
                    .entry(NormalizedKey::new(path))
                    .or_default()
                    .push(idx);
            }
        }

        Self {
            sources: source_paths,
            path_to_sources,
        }
    }

    /// Returns all normalized keys in sorted order.
    #[must_use]
    pub fn keys(&self) -> Vec<PathBuf> {
        let mut keys: Vec<PathBuf> = self
            .path_to_sources
            .keys()
            .cloned()
            .map(NormalizedKey::into_path_buf)
            .collect();
        keys.sort();
        keys
    }

    /// Returns the stable source ID for `path` if present.
    #[must_use]
    pub fn source_id_for_path(&self, path: &Path) -> Option<SourceId> {
        self.sources
            .iter()
            .position(|meta| meta.path == path)
            .map(SourceId::from_index)
    }

    /// Returns source metadata for a stable source ID.
    #[must_use]
    pub fn source_by_id(&self, source_id: SourceId) -> Option<&SourceMeta> {
        self.sources.get(source_id.as_index())
    }

    /// Returns source indices that provide `path`, in load order.
    pub fn sources_containing(&self, path: &Path) -> &[usize] {
        let normalized = NormalizedKey::new(path);
        self.path_to_sources
            .get(&normalized)
            .map_or(&[], Vec::as_slice)
    }

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

        let Some(winner_idx) = provider_indices.last().copied() else {
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
                SourceKind::LooseDir => src.path.join(&key).display().to_string(),
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
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> =
            AHashMap::new();

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

        let mut entries = Vec::new();
        for key in keys {
            if let Some(entry) = self.semantic_conflict_for_key(vfs, &key, opts, &mut hash_cache)? {
                entries.push(entry);
            }
        }

        Ok(SemanticConflictReport { entries })
    }

    fn semantic_conflict_for_key(
        &self,
        vfs: &VFS,
        key: &Path,
        opts: SemanticOpts,
        hash_cache: &mut AHashMap<(usize, PathBuf), Option<ContentFingerprint>>,
    ) -> io::Result<Option<SemanticConflict>> {
        let provider_indices = self.sources_containing(key);
        if provider_indices.len() < 2 {
            return Ok(None);
        }

        let Some(winner_idx) = provider_indices.last().copied() else {
            return Ok(None);
        };

        let winner_source = self.sources[winner_idx].clone();
        let winner_fp = self.fingerprint_for_provider(
            vfs,
            winner_idx,
            key,
            hash_cache,
            opts.archive_hash_mode,
        )?;
        let winner_bytes = if opts.include_semantic_deltas {
            self.read_provider_bytes(vfs, winner_idx, key)
        } else {
            Ok(None)
        }?;

        let mut seen_hashes = AHashSet::<String>::new();
        let mut providers = Vec::with_capacity(provider_indices.len());
        let mut inferred_asset_class = AssetClass::Unknown;

        for &idx in provider_indices {
            let src = self.sources[idx].clone();
            let current =
                self.fingerprint_for_provider(vfs, idx, key, hash_cache, opts.archive_hash_mode)?;

            let semantic_delta_to_winner = if opts.include_semantic_deltas {
                let current_bytes = self.read_provider_bytes(vfs, idx, key)?;
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
            all_identical: !seen_hashes.is_empty() && seen_hashes.len() == 1,
            distinct_versions: seen_hashes.len(),
        }))
    }

    /// Build deterministic lock manifest from current winners.
    ///
    /// # Errors
    ///
    /// Returns an error when reading winner file content for hashing fails.
    pub fn lock_manifest(&self, vfs: &VFS) -> io::Result<VfsLock> {
        let mut entries = Vec::new();
        let mut hash_cache: AHashMap<(usize, PathBuf), Option<ContentFingerprint>> =
            AHashMap::new();

        for key in self.keys() {
            let providers = self.sources_containing(&key);
            if providers.is_empty() {
                continue;
            }
            let Some(winner_idx) = providers.last().copied() else {
                continue;
            };
            let winner_source = &self.sources[winner_idx];
            let winner_fp = self.fingerprint_for_provider(
                vfs,
                winner_idx,
                &key,
                &mut hash_cache,
                ArchiveHashMode::WinnerOnly,
            )?;

            entries.push(VfsLockEntry {
                key,
                winner_source: winner_source.path.clone(),
                winner_kind: winner_source.kind,
                winner_hash_blake3: winner_fp.as_ref().map(|f| f.to_digest().hex),
                winner_size: winner_fp.as_ref().map(|f| f.to_digest().size),
                provider_count: providers.len(),
            });
        }

        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(VfsLock {
            schema_version: 1,
            entries,
        })
    }

    /// Simulate a simple load-order edit and report winner deltas.
    ///
    /// # Errors
    ///
    /// Returns an error when reorder operation parameters are invalid.
    pub fn simulate(&self, vfs: &VFS, op: ReorderOp) -> io::Result<SimulationDelta> {
        let opts = SimOpts::default();
        self.simulate_with_opts(vfs, op, &opts)
    }

    /// Simulate a load-order edit and report winner deltas.
    ///
    /// # Errors
    ///
    /// Returns an error when reorder operation parameters are invalid.
    pub fn simulate_with_opts(
        &self,
        vfs: &VFS,
        op: ReorderOp,
        opts: &SimOpts,
    ) -> io::Result<SimulationDelta> {
        let order = self.reordered_indices(op)?;
        let rank_by_source = self.rank_by_source(&order);

        let mut wins_before = vec![0usize; self.sources.len()];
        let mut wins_after = vec![0usize; self.sources.len()];
        let mut changed = Vec::<PathBuf>::new();

        for key in self.keys() {
            let providers = self.sources_containing(&key);
            if providers.is_empty() {
                continue;
            }

            let before_idx = self.current_winner_source_idx(vfs, &key, providers);
            let Some(after_idx) = providers
                .iter()
                .copied()
                .max_by_key(|idx| rank_by_source[*idx])
            else {
                continue;
            };

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

        let bucket_rows = opts
            .impact_buckets
            .iter()
            .map(|bucket| BucketDelta {
                bucket: bucket.clone(),
                changed_winners: changed
                    .iter()
                    .filter(|key| path_glob_matches(bucket, key))
                    .count(),
            })
            .collect();

        Ok(SimulationDelta {
            changed_winners: changed.len(),
            unchanged_winners: wins_after.iter().sum::<usize>() - changed.len(),
            by_source_gain_loss: rows,
            by_bucket: bucket_rows,
            changed_keys_sample: changed.into_iter().take(opts.sample_limit).collect(),
        })
    }

    /// Simulate a reordering operation and compute heuristic impact scores.
    ///
    /// # Errors
    ///
    /// Returns an error when simulation or semantic analysis fails.
    pub fn simulate_impact(
        &self,
        vfs: &VFS,
        op: ReorderOp,
        opts: &SimOpts,
        profile: &ImpactProfile,
    ) -> io::Result<ImpactReport> {
        let mut all_opts = opts.clone();
        all_opts.sample_limit = usize::MAX;
        let delta = self.simulate_with_opts(vfs, op, &all_opts)?;

        let semantic = self.semantic_conflicts_with_opts(
            vfs,
            SemanticOpts {
                include_semantic_deltas: true,
                ..SemanticOpts::default()
            },
        )?;
        let behavior_changing: AHashSet<PathBuf> = semantic
            .entries
            .into_iter()
            .filter_map(|entry| {
                let has_behavior_change = entry.providers.iter().any(|provider| {
                    matches!(
                        provider.semantic_delta_to_winner,
                        Some(SemanticDelta::BehaviorChanging { .. })
                    )
                });
                if has_behavior_change {
                    Some(entry.key)
                } else {
                    None
                }
            })
            .collect();

        let mut scored = Vec::<RiskyChange>::new();
        for key in delta.changed_keys_sample {
            let mut score = 0.0_f32;
            let mut reasons = Vec::new();

            for heuristic in &profile.heuristics {
                if !path_glob_matches(&heuristic.path_glob, &key) {
                    continue;
                }
                let condition_matches = match heuristic.condition {
                    HeuristicCondition::WinnerChanged => true,
                    HeuristicCondition::WinnerChangedAndSemanticBehaviorChanging => {
                        behavior_changing.contains(&key)
                    }
                };
                if condition_matches {
                    score += heuristic.weight;
                    reasons.push(heuristic.name.clone());
                }
            }

            if score > 0.0 {
                scored.push(RiskyChange {
                    key,
                    score,
                    reasons,
                });
            }
        }

        scored.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.key.cmp(&b.key)));

        let by_bucket = opts
            .impact_buckets
            .iter()
            .map(|bucket| BucketImpact {
                bucket: bucket.clone(),
                score: scored
                    .iter()
                    .filter(|change| path_glob_matches(bucket, &change.key))
                    .map(|change| change.score)
                    .sum(),
            })
            .collect();

        let overall_score: f32 = scored.iter().map(|change| change.score).sum();
        let risk_level = if overall_score == 0.0 {
            RiskLevel::Low
        } else if overall_score < 5.0 {
            RiskLevel::Medium
        } else if overall_score < 15.0 {
            RiskLevel::High
        } else {
            RiskLevel::Critical
        };

        Ok(ImpactReport {
            overall_score,
            risk_level,
            by_bucket,
            top_risky_changes: scored.into_iter().take(100).collect(),
        })
    }

    /// Compare current VFS state against a lock manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when building current lock state fails.
    pub fn diff_against_lock(&self, vfs: &VFS, expected: &VfsLock) -> io::Result<DriftReport> {
        let current = self.lock_manifest(vfs)?;

        let mut expected_map: AHashMap<PathBuf, &VfsLockEntry> = AHashMap::new();
        for row in &expected.entries {
            expected_map.insert(row.key.clone(), row);
        }

        let mut current_map: AHashMap<PathBuf, &VfsLockEntry> = AHashMap::new();
        for row in &current.entries {
            current_map.insert(row.key.clone(), row);
        }

        let mut entries = Vec::<DriftEntry>::new();

        for key in current_map.keys() {
            if !expected_map.contains_key(key) {
                entries.push(DriftEntry {
                    key: key.clone(),
                    kind: DriftKind::Added,
                });
            }
        }

        for key in expected_map.keys() {
            if !current_map.contains_key(key) {
                entries.push(DriftEntry {
                    key: key.clone(),
                    kind: DriftKind::Removed,
                });
            }
        }

        for (key, expected_row) in &expected_map {
            let Some(current_row) = current_map.get(key) else {
                continue;
            };

            if expected_row.winner_source != current_row.winner_source {
                entries.push(DriftEntry {
                    key: (*key).clone(),
                    kind: DriftKind::WinnerSourceChanged,
                });
            }

            if expected_row.winner_hash_blake3 != current_row.winner_hash_blake3 {
                entries.push(DriftEntry {
                    key: (*key).clone(),
                    kind: DriftKind::WinnerHashChanged,
                });
            }

            if expected_row.provider_count != current_row.provider_count {
                entries.push(DriftEntry {
                    key: (*key).clone(),
                    kind: DriftKind::ProviderCountChanged,
                });
            }
        }

        entries.sort_by(|a, b| a.key.cmp(&b.key).then(a.kind.cmp(&b.kind)));
        let mut counts: BTreeMap<DriftKind, usize> = BTreeMap::new();
        for entry in &entries {
            *counts.entry(entry.kind).or_insert(0) += 1;
        }

        Ok(DriftReport { entries, counts })
    }

    /// Build a preflight plan for adding one candidate directory on top of current VFS.
    ///
    /// # Errors
    ///
    /// Returns an error when candidate or winner file hashing fails.
    pub fn plan_candidate_directory(
        &self,
        vfs: &VFS,
        candidate_dir: &Path,
        opts: CandidatePlanOpts,
    ) -> io::Result<CandidatePlan> {
        let diff = vfs.diff_directory(candidate_dir);

        let mut additions = diff
            .additions
            .into_iter()
            .map(|(key, _incoming)| key)
            .collect::<Vec<_>>();
        additions.sort();

        let mut conflicts = Vec::new();
        let mut displaced_winners = Vec::new();

        for (key, incoming, existing) in diff.conflicts {
            let providers = self.sources_containing(&key);
            let current_winner_source = self
                .current_winner_source_idx(vfs, &key, providers)
                .map(|idx| self.sources[idx].path.clone())
                .unwrap_or_default();

            let semantic_differs = if opts.include_semantic {
                let incoming_fp = hash_reader(std::fs::File::open(incoming.path())?)?;
                let existing_fp = hash_reader(existing.open()?)?;
                Some(incoming_fp.digest != existing_fp.digest)
            } else {
                None
            };

            displaced_winners.push(key.clone());
            conflicts.push(CandidateConflict {
                key,
                current_winner_source,
                candidate_file: incoming.path().to_path_buf(),
                semantic_differs,
                risk: None,
            });
        }

        conflicts.sort_by(|a, b| a.key.cmp(&b.key));
        displaced_winners.sort();

        Ok(CandidatePlan {
            summary: CandidatePlanSummary {
                additions: additions.len(),
                conflicts: conflicts.len(),
                displaced_winners: displaced_winners.len(),
            },
            additions,
            conflicts,
            displaced_winners,
        })
    }

    fn reordered_indices(&self, op: ReorderOp) -> io::Result<Vec<usize>> {
        let mut order: Vec<usize> = (0..self.sources.len()).collect();
        match op {
            ReorderOp::Swap(a, b) => {
                let ai = self
                    .sources
                    .iter()
                    .position(|s| s.path == a)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "swap source A not found")
                    })?;
                let bi = self
                    .sources
                    .iter()
                    .position(|s| s.path == b)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "swap source B not found")
                    })?;
                order.swap(ai, bi);
            }
            ReorderOp::MoveBefore { source, before } => {
                let src_idx = self
                    .sources
                    .iter()
                    .position(|s| s.path == source)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "move source not found")
                    })?;
                let dst_idx = self
                    .sources
                    .iter()
                    .position(|s| s.path == before)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "before source not found")
                    })?;
                let item = order.remove(src_idx);
                let insert_at = if src_idx < dst_idx {
                    dst_idx - 1
                } else {
                    dst_idx
                };
                order.insert(insert_at, item);
            }
            ReorderOp::MoveAfter { source, after } => {
                let src_idx = self
                    .sources
                    .iter()
                    .position(|s| s.path == source)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "move source not found")
                    })?;
                let dst_idx = self
                    .sources
                    .iter()
                    .position(|s| s.path == after)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "after source not found")
                    })?;
                let item = order.remove(src_idx);
                let insert_at = if src_idx < dst_idx {
                    dst_idx
                } else {
                    dst_idx + 1
                };
                order.insert(insert_at, item);
            }
            ReorderOp::FullOrder(paths) => {
                if paths.len() != self.sources.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "full-order path count does not match source count",
                    ));
                }
                let mut seen = AHashSet::new();
                let mut ordered = Vec::with_capacity(paths.len());
                for path in paths {
                    let idx = self
                        .sources
                        .iter()
                        .position(|s| s.path == path)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                format!("unknown source in full order: {}", path.display()),
                            )
                        })?;
                    if !seen.insert(idx) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("duplicate source in full order: {}", path.display()),
                        ));
                    }
                    ordered.push(idx);
                }
                order = ordered;
            }
        }
        Ok(order)
    }

    fn rank_by_source(&self, order: &[usize]) -> Vec<usize> {
        let mut ranks = vec![0usize; self.sources.len()];
        for (rank, src_idx) in order.iter().enumerate() {
            ranks[*src_idx] = rank;
        }
        ranks
    }

    fn current_winner_source_idx(
        &self,
        vfs: &VFS,
        key: &Path,
        providers: &[usize],
    ) -> Option<usize> {
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
        archive_hash_mode: ArchiveHashMode,
    ) -> io::Result<Option<ContentFingerprint>> {
        let cache_key = (source_idx, key.to_path_buf());
        if let Some(hit) = cache.get(&cache_key) {
            return Ok(hit.clone());
        }

        let src = &self.sources[source_idx];
        let fp = match src.kind {
            SourceKind::LooseDir => {
                let path = src.path.join(key);
                if path.exists() {
                    Some(hash_reader(std::fs::File::open(path)?)?)
                } else {
                    None
                }
            }
            SourceKind::Archive => match archive_hash_mode {
                ArchiveHashMode::Disabled => None,
                ArchiveHashMode::WinnerOnly | ArchiveHashMode::AllProviders => {
                    match vfs.get_file(key) {
                        Some(current_winner) => match current_winner.parent_archive_path() {
                            Some(parent) if parent == src.path.to_string_lossy() => {
                                Some(hash_reader(current_winner.open()?)?)
                            }
                            _ => None,
                        },
                        None => None,
                    }
                }
            },
        };

        cache.insert(cache_key, fp.clone());
        Ok(fp)
    }

    fn read_provider_bytes(
        &self,
        vfs: &VFS,
        source_idx: usize,
        key: &Path,
    ) -> io::Result<Option<Vec<u8>>> {
        let src = &self.sources[source_idx];
        let mut out = Vec::new();

        match src.kind {
            SourceKind::LooseDir => {
                let path = src.path.join(key);
                if !path.exists() {
                    return Ok(None);
                }
                let mut file = std::fs::File::open(path)?;
                file.read_to_end(&mut out)?;
                Ok(Some(out))
            }
            SourceKind::Archive => {
                let Some(winner) = vfs.get_file(key) else {
                    return Ok(None);
                };
                let Some(parent) = winner.parent_archive_path() else {
                    return Ok(None);
                };
                if parent != src.path.to_string_lossy() {
                    return Ok(None);
                }

                let mut reader = winner.open()?;
                reader.read_to_end(&mut out)?;
                Ok(Some(out))
            }
        }
    }
}

fn hash_reader(mut reader: impl Read) -> io::Result<ContentFingerprint> {
    let mut hasher = blake3::Hasher::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; 65536];

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
        let lock = index
            .lock_manifest(&vfs)
            .expect("lock manifest should succeed");
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

    #[test]
    fn simulate_move_before_changes_winner() {
        let a = TempDir::new("analysis_sim_move_before_a");
        let b = TempDir::new("analysis_sim_move_before_b");
        let c = TempDir::new("analysis_sim_move_before_c");
        a.write("textures/a.dds", b"a");
        b.write("textures/a.dds", b"b");
        c.write("textures/a.dds", b"c");

        let (vfs, index) =
            VFS::from_directories_with_layer_index([a.path(), b.path(), c.path()], None);
        let delta = index
            .simulate(
                &vfs,
                ReorderOp::MoveBefore {
                    source: c.path().to_path_buf(),
                    before: a.path().to_path_buf(),
                },
            )
            .expect("simulate move-before should succeed");

        assert_eq!(delta.changed_winners, 1);
    }

    #[test]
    fn simulate_move_after_changes_winner() {
        let a = TempDir::new("analysis_sim_move_after_a");
        let b = TempDir::new("analysis_sim_move_after_b");
        a.write("textures/a.dds", b"a");
        b.write("textures/a.dds", b"b");

        let (vfs, index) = VFS::from_directories_with_layer_index([a.path(), b.path()], None);
        let delta = index
            .simulate(
                &vfs,
                ReorderOp::MoveAfter {
                    source: a.path().to_path_buf(),
                    after: b.path().to_path_buf(),
                },
            )
            .expect("simulate move-after should succeed");

        assert_eq!(delta.changed_winners, 1);
    }

    #[test]
    fn simulate_full_order_rejects_duplicate_sources() {
        let a = TempDir::new("analysis_sim_full_dup_a");
        let b = TempDir::new("analysis_sim_full_dup_b");
        a.write("textures/a.dds", b"a");
        b.write("textures/a.dds", b"b");

        let (vfs, index) = VFS::from_directories_with_layer_index([a.path(), b.path()], None);
        let err = index
            .simulate(
                &vfs,
                ReorderOp::FullOrder(vec![a.path().to_path_buf(), a.path().to_path_buf()]),
            )
            .expect_err("duplicate full-order should error");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn simulate_with_buckets_reports_counts() {
        let low = TempDir::new("analysis_sim_bucket_low");
        let high = TempDir::new("analysis_sim_bucket_high");
        low.write("textures/a.dds", b"low");
        high.write("textures/a.dds", b"high");
        low.write("meshes/a.nif", b"low");
        high.write("meshes/a.nif", b"high");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let opts = SimOpts {
            sample_limit: 10,
            impact_buckets: vec!["textures/**".into(), "meshes/**".into()],
        };
        let delta = index
            .simulate_with_opts(
                &vfs,
                ReorderOp::Swap(low.path().to_path_buf(), high.path().to_path_buf()),
                &opts,
            )
            .expect("simulate with buckets should succeed");

        assert_eq!(delta.by_bucket.len(), 2);
        assert_eq!(delta.by_bucket[0].changed_winners, 1);
        assert_eq!(delta.by_bucket[1].changed_winners, 1);
    }

    #[test]
    fn simulate_impact_scores_with_profile() {
        let low = TempDir::new("analysis_impact_low");
        let high = TempDir::new("analysis_impact_high");
        low.write("scripts/x.lua", b"print('a')\n");
        high.write("scripts/x.lua", b"print('b')\n");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let opts = SimOpts {
            sample_limit: 50,
            impact_buckets: vec!["scripts/**".into()],
        };
        let profile = ImpactProfile {
            heuristics: vec![ImpactHeuristic {
                name: "scripts-change".into(),
                path_glob: "scripts/**".into(),
                weight: 3.0,
                condition: HeuristicCondition::WinnerChanged,
            }],
        };

        let report = index
            .simulate_impact(
                &vfs,
                ReorderOp::Swap(low.path().to_path_buf(), high.path().to_path_buf()),
                &opts,
                &profile,
            )
            .expect("simulate impact should succeed");

        assert!(report.overall_score > 0.0);
        assert!(!report.top_risky_changes.is_empty());
        assert_eq!(report.by_bucket.len(), 1);
    }

    #[test]
    fn drift_detects_source_and_hash_changes() {
        let low = TempDir::new("analysis_drift_low");
        let high = TempDir::new("analysis_drift_high");
        low.write("textures/a.dds", b"aaa");
        high.write("textures/a.dds", b"bbb");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let mut lock = index
            .lock_manifest(&vfs)
            .expect("lock build should succeed");

        lock.entries[0].winner_source = low.path().to_path_buf();
        lock.entries[0].winner_hash_blake3 = Some("00".repeat(32));

        let drift = index
            .diff_against_lock(&vfs, &lock)
            .expect("drift diff should succeed");

        assert!(
            drift
                .entries
                .iter()
                .any(|entry| entry.kind == DriftKind::WinnerSourceChanged)
        );
        assert!(
            drift
                .entries
                .iter()
                .any(|entry| entry.kind == DriftKind::WinnerHashChanged)
        );
    }

    #[test]
    fn candidate_plan_reports_additions_and_conflicts() {
        let low = TempDir::new("analysis_plan_low");
        let high = TempDir::new("analysis_plan_high");
        let candidate = TempDir::new("analysis_plan_candidate");

        low.write("textures/a.dds", b"low");
        high.write("textures/a.dds", b"high");
        high.write("textures/b.dds", b"b");

        candidate.write("textures/a.dds", b"candidate-new");
        candidate.write("textures/c.dds", b"c");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let plan = index
            .plan_candidate_directory(&vfs, candidate.path(), CandidatePlanOpts::default())
            .expect("candidate plan should succeed");

        assert_eq!(plan.summary.additions, 1);
        assert_eq!(plan.summary.conflicts, 1);
        assert_eq!(plan.summary.displaced_winners, 1);
        assert_eq!(plan.additions[0], PathBuf::from("textures/c.dds"));
        assert_eq!(plan.conflicts[0].key, PathBuf::from("textures/a.dds"));
        assert_eq!(plan.conflicts[0].semantic_differs, Some(true));
    }

    #[test]
    fn candidate_plan_semantic_can_be_disabled() {
        let base = TempDir::new("analysis_plan_semantic_base");
        let candidate = TempDir::new("analysis_plan_semantic_candidate");
        base.write("textures/a.dds", b"same");
        candidate.write("textures/a.dds", b"same");

        let (vfs, index) = VFS::from_directories_with_layer_index([base.path()], None);
        let plan = index
            .plan_candidate_directory(
                &vfs,
                candidate.path(),
                CandidatePlanOpts {
                    include_semantic: false,
                },
            )
            .expect("candidate plan should succeed");

        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].semantic_differs, None);
    }

    #[test]
    fn semantic_conflicts_enrich_adds_asset_class_and_delta() {
        let low = TempDir::new("analysis_semantic_enrich_low");
        let high = TempDir::new("analysis_semantic_enrich_high");

        low.write("config/example.ini", b"[sec]\na=1\nb=2\n");
        high.write("config/example.ini", b"# comment\n[sec]\nb=2\na=1\n");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let report = index
            .semantic_conflicts_with_opts(
                &vfs,
                SemanticOpts {
                    include_semantic_deltas: true,
                    ..SemanticOpts::default()
                },
            )
            .expect("semantic enrich should succeed");

        let entry = report
            .entries
            .iter()
            .find(|entry| entry.key == Path::new("config/example.ini"))
            .expect("expected example.ini conflict entry");

        assert_eq!(entry.asset_class, AssetClass::Ini);
        assert!(
            entry
                .providers
                .iter()
                .all(|provider| provider.semantic_delta_to_winner.is_some())
        );
        assert!(entry.providers.iter().any(|provider| {
            provider.semantic_delta_to_winner == Some(SemanticDelta::CosmeticOnly)
        }));
    }
}
