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
mod tests {
    use super::*;
    use crate::VFS;
    use std::fs;
    use std::path::Path;

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
    fn layer_index_deduplicates_keys_within_one_source() {
        let index = LayerIndex::from_file_lists(vec![(
            SourceMeta {
                path: PathBuf::from("/one"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("shared.txt"), PathBuf::from("SHARED.TXT")],
        )]);

        assert_eq!(index.sources_containing(Path::new("shared.txt")), &[0]);
    }

    #[test]
    fn layer_index_includes_unique_provider_keys() {
        let data = TempDir::new("analysis_unique_provider");
        data.write("unique.txt", b"unique");
        let (_vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);

        assert_eq!(index.sources_containing(Path::new("unique.txt")), &[0]);
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
    fn lock_manifest_hashes_mixed_case_loose_winner_path() {
        let data = TempDir::new("analysis_lock_mixed_case");
        data.write("Textures/Foo.DDS", b"mixed");

        let (vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);
        let lock = index.lock_manifest(&vfs).expect("lock should build");

        assert_eq!(lock.entries[0].key, PathBuf::from("textures/foo.dds"));
        assert_eq!(lock.entries[0].winner_size, Some(5));
        assert!(lock.entries[0].winner_hash_blake3.is_some());
    }

    #[test]
    fn semantic_conflicts_reads_mixed_case_loose_provider_paths() {
        let low = TempDir::new("analysis_semantic_mixed_low");
        let high = TempDir::new("analysis_semantic_mixed_high");
        low.write("Textures/Foo.DDS", b"low");
        high.write("textures/foo.dds", b"high");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let report = index.semantic_conflicts(&vfs).expect("semantic report");
        let entry = report
            .entries
            .iter()
            .find(|entry| entry.key == Path::new("textures/foo.dds"))
            .expect("mixed-case conflict should be reported");

        assert_eq!(entry.distinct_versions, 2);
        assert!(
            entry
                .providers
                .iter()
                .all(|provider| provider.hash_blake3.is_some())
        );
    }

    #[test]
    fn semantic_conflict_omits_key_without_actual_vfs_winner() {
        let low = TempDir::new("analysis_semantic_no_winner_hash_low");
        low.write("shared.txt", b"same");
        let index = LayerIndex::from_file_lists(vec![
            (
                SourceMeta {
                    path: low.path().to_path_buf(),
                    kind: SourceKind::LooseDir,
                },
                vec![PathBuf::from("shared.txt")],
            ),
            (
                SourceMeta {
                    path: PathBuf::from("missing.bsa"),
                    kind: SourceKind::Archive,
                },
                vec![PathBuf::from("shared.txt")],
            ),
        ]);
        let vfs = VFS::new();

        let entry = index
            .semantic_conflict_for_key_no_cache(
                &vfs,
                Path::new("shared.txt"),
                SemanticOpts::default(),
            )
            .expect("semantic conflict should build");

        assert!(entry.is_none());
    }

    #[test]
    #[cfg(feature = "zip")]
    fn semantic_conflicts_all_providers_hashes_zip_archives() {
        use std::io::Write as _;

        fn write_zip(path: &Path, entry: &str, data: &[u8]) {
            let file = fs::File::create(path).expect("zip file should be created");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer
                .start_file(entry, options)
                .expect("entry should start");
            writer.write_all(data).expect("entry should be written");
            writer.finish().expect("zip should finish");
        }

        let data = TempDir::new("analysis_semantic_zip_all_providers");
        write_zip(&data.path().join("low.zip"), "Textures/Foo.DDS", b"low");
        write_zip(&data.path().join("high.zip"), "textures/foo.dds", b"high");

        let (vfs, index) = VFS::from_directories_with_layer_index(
            [data.path()],
            Some(vec!["low.zip", "high.zip"]),
        );
        let report = index
            .semantic_conflicts_with_opts(
                &vfs,
                SemanticOpts {
                    archive_hash_mode: ArchiveHashMode::AllProviders,
                    include_semantic_deltas: false,
                },
            )
            .expect("semantic report should build");
        let entry = report
            .entries
            .iter()
            .find(|entry| entry.key == Path::new("textures/foo.dds"))
            .expect("archive conflict should be reported");

        assert_eq!(entry.distinct_versions, 2);
        assert!(
            entry
                .providers
                .iter()
                .all(|provider| provider.hash_blake3.is_some())
        );
    }

    #[test]
    fn lock_manifest_uses_actual_vfs_winner_presence() {
        let low = TempDir::new("analysis_lock_removed_low");
        let high = TempDir::new("analysis_lock_removed_high");
        low.write("shared.txt", b"low");
        high.write("shared.txt", b"high");

        let (mut vfs, index) =
            VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        vfs.remove_file("shared.txt");

        let lock = index.lock_manifest(&vfs).expect("lock should build");
        assert!(lock.entries.is_empty());
    }

    #[test]
    fn lock_manifest_is_deterministic_across_runs() {
        let low = TempDir::new("analysis_lock_deterministic_low");
        let high = TempDir::new("analysis_lock_deterministic_high");
        low.write("textures/a.dds", b"aaa");
        high.write("textures/a.dds", b"bbb");
        low.write("meshes/m.nif", b"m");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let first = index
            .lock_manifest(&vfs)
            .expect("first lock build should succeed");
        let second = index
            .lock_manifest(&vfs)
            .expect("second lock build should succeed");

        let first_rows = first
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.key.clone(),
                    entry.winner_source.clone(),
                    entry.winner_kind,
                    entry.winner_hash_blake3.clone(),
                    entry.winner_size,
                    entry.provider_count,
                )
            })
            .collect::<Vec<_>>();
        let second_rows = second
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.key.clone(),
                    entry.winner_source.clone(),
                    entry.winner_kind,
                    entry.winner_hash_blake3.clone(),
                    entry.winner_size,
                    entry.provider_count,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(first_rows, second_rows);
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
    fn simulate_reorder_preserves_loose_over_archive_precedence() {
        let loose = TempDir::new("analysis_sim_loose_archive_loose");
        loose.write("textures/a.dds", b"loose");
        let loose_file = loose.path().join("textures/a.dds");
        let archive = PathBuf::from("/archives/base.bsa");
        let mut vfs = VFS::new();
        vfs.insert_loose_file("textures/a.dds", loose_file);
        let index = LayerIndex::from_file_lists(vec![
            (
                SourceMeta {
                    path: loose.path().to_path_buf(),
                    kind: SourceKind::LooseDir,
                },
                vec![PathBuf::from("textures/a.dds")],
            ),
            (
                SourceMeta {
                    path: archive.clone(),
                    kind: SourceKind::Archive,
                },
                vec![PathBuf::from("textures/a.dds")],
            ),
        ]);

        let delta = index
            .simulate(
                &vfs,
                ReorderOp::FullOrder(vec![loose.path().to_path_buf(), archive]),
            )
            .expect("simulation should succeed");

        assert_eq!(delta.changed_winners, 0);
        assert_eq!(delta.by_source_gain_loss[0].wins_after, 1);
        assert_eq!(delta.by_source_gain_loss[1].wins_after, 0);
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
    fn simulate_impact_semantic_score_uses_before_after_winners_only() {
        let low = TempDir::new("analysis_impact_semantic_low");
        let mid = TempDir::new("analysis_impact_semantic_mid");
        let high = TempDir::new("analysis_impact_semantic_high");
        low.write("config/test.json", br#"{"value":2}"#);
        mid.write("config/test.json", br#"{"value":1}"#);
        high.write(
            "config/test.json",
            br#"{
  "value": 1
}"#,
        );

        let (vfs, index) =
            VFS::from_directories_with_layer_index([low.path(), mid.path(), high.path()], None);
        let opts = SimOpts {
            sample_limit: 50,
            impact_buckets: vec!["config/**".into()],
        };
        let profile = ImpactProfile {
            heuristics: vec![ImpactHeuristic {
                name: "semantic-change".into(),
                path_glob: "config/**".into(),
                weight: 5.0,
                condition: HeuristicCondition::WinnerChangedAndSemanticBehaviorChanging,
            }],
        };

        let report = index
            .simulate_impact(
                &vfs,
                ReorderOp::Swap(mid.path().to_path_buf(), high.path().to_path_buf()),
                &opts,
                &profile,
            )
            .expect("simulate impact should succeed");

        assert!(report.overall_score.abs() <= f32::EPSILON);
        assert!(report.top_risky_changes.is_empty());
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
    fn candidate_plan_deduplicates_normalized_candidate_keys() {
        let base = TempDir::new("analysis_plan_dedupe_base");
        let candidate = TempDir::new("analysis_plan_dedupe_candidate");
        base.write("textures/a.dds", b"base");
        candidate.write("textures/a.dds", b"candidate-lower");
        candidate.write("Textures/A.dds", b"candidate-upper");
        candidate.write("meshes/b.nif", b"candidate-lower");
        candidate.write("Meshes/B.nif", b"candidate-upper");

        let (vfs, index) = VFS::from_directories_with_layer_index([base.path()], None);
        let plan = index
            .plan_candidate_directory(&vfs, candidate.path(), CandidatePlanOpts::default())
            .expect("candidate plan should succeed");

        assert_eq!(plan.summary.additions, 1);
        assert_eq!(plan.summary.conflicts, 1);
        assert_eq!(plan.summary.displaced_winners, 1);
        assert_eq!(plan.additions, vec![PathBuf::from("meshes/b.nif")]);
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].key, PathBuf::from("textures/a.dds"));
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

    #[test]
    fn semantic_conflicts_are_deterministic_across_runs() {
        let low = TempDir::new("analysis_semantic_deterministic_low");
        let high = TempDir::new("analysis_semantic_deterministic_high");
        low.write("scripts/a.lua", b"print('a')\n");
        high.write("scripts/a.lua", b"print('b')\n");
        low.write("config/example.ini", b"[sec]\na=1\n");
        high.write("config/example.ini", b"[sec]\na=2\n");

        let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
        let opts = SemanticOpts {
            include_semantic_deltas: true,
            ..SemanticOpts::default()
        };
        let first = index
            .semantic_conflicts_with_opts(&vfs, opts)
            .expect("first semantic report should succeed");
        let second = index
            .semantic_conflicts_with_opts(&vfs, opts)
            .expect("second semantic report should succeed");

        let first_rows = first
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.key.clone(),
                    entry.winner.path.clone(),
                    entry.winner.kind,
                    entry.asset_class,
                    entry.all_identical,
                    entry.distinct_versions,
                    entry
                        .providers
                        .iter()
                        .map(|provider| {
                            (
                                provider.source.path.clone(),
                                provider.source.kind,
                                provider.relation,
                                provider.hash_blake3.clone(),
                                provider.size,
                                provider.semantic_delta_to_winner.clone(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let second_rows = second
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.key.clone(),
                    entry.winner.path.clone(),
                    entry.winner.kind,
                    entry.asset_class,
                    entry.all_identical,
                    entry.distinct_versions,
                    entry
                        .providers
                        .iter()
                        .map(|provider| {
                            (
                                provider.source.path.clone(),
                                provider.source.kind,
                                provider.relation,
                                provider.hash_blake3.clone(),
                                provider.size,
                                provider.semantic_delta_to_winner.clone(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(first_rows, second_rows);
    }
}
