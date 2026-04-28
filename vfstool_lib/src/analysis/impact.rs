// SPDX-License-Identifier: GPL-3.0-only
use super::provider_io::ProviderIoCache;
use super::{
    BucketImpact, HeuristicCondition, ImpactProfile, ImpactReport, LayerIndex, ReorderOp,
    RiskLevel, RiskyChange, SimOpts,
};
use crate::{
    VFS, path_glob_matches,
    paths::key_to_path_buf_lossy,
    semantic::{SemanticDelta, analyze_pair},
};
use ahash::AHashSet;
use std::{io, path::PathBuf};

impl LayerIndex {
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
        let delta = self.simulate_with_opts(vfs, op.clone(), &all_opts)?;

        let needs_semantic = profile.heuristics.iter().any(|heuristic| {
            heuristic.condition == HeuristicCondition::WinnerChangedAndSemanticBehaviorChanging
        });
        let behavior_changing = if needs_semantic {
            self.behavior_changing_after_reorder(vfs, op)?
        } else {
            AHashSet::new()
        };

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

    fn behavior_changing_after_reorder(
        &self,
        vfs: &VFS,
        op: ReorderOp,
    ) -> io::Result<AHashSet<PathBuf>> {
        let order = self.reordered_indices(op)?;
        let rank_by_source = self.rank_by_source(&order);
        let mut behavior_changing = AHashSet::new();
        let mut provider_cache = ProviderIoCache::new();

        for key in self.keys() {
            let providers = self.sources_containing(&key);
            let before_idx = Self::current_winner_source_idx(vfs, &key, providers);
            let Some(after_idx) = self.winner_after_reorder(providers, &rank_by_source) else {
                continue;
            };
            let Some(before_idx) = before_idx else {
                continue;
            };
            if before_idx == after_idx {
                continue;
            }

            let before_bytes =
                self.read_provider_bytes(vfs, before_idx, &key, &mut provider_cache)?;
            let after_bytes =
                self.read_provider_bytes(vfs, after_idx, &key, &mut provider_cache)?;
            if let (Some(before), Some(after)) = (before_bytes, after_bytes) {
                let key_path = key_to_path_buf_lossy(&key);
                let (_, delta) = analyze_pair(&key_path, &after, &before);
                if matches!(delta, SemanticDelta::BehaviorChanging { .. }) {
                    behavior_changing.insert(key_path);
                }
            }
        }

        Ok(behavior_changing)
    }
}
