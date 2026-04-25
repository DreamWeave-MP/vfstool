// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{
    NormalizedKey,
    semantic::{AssetClass, SemanticDelta},
};
use ahash::AHashMap;
use std::{collections::BTreeMap, path::PathBuf};

mod candidate;
mod drift;
mod impact;
mod layer;
mod lock;
mod provenance;
mod provider_io;
mod semantic_conflicts;
mod simulate;

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
    provider_paths: AHashMap<(usize, NormalizedKey), PathBuf>,
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
#[cfg(test)]
#[path = "analysis/tests.rs"]
mod tests;
