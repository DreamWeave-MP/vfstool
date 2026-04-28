// SPDX-License-Identifier: GPL-3.0-only
use super::{
    BucketDelta, LayerIndex, ReorderOp, SimOpts, SimulationDelta, SourceDelta, SourceKind,
};
use crate::{
    NormalizedPath, VFS, normalize_host_path, path_glob_matches, paths::key_to_path_buf_lossy,
};
use ahash::AHashSet;
use std::{io, path::Path};

impl LayerIndex {
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
        let mut changed = Vec::new();

        for key in self.keys() {
            let providers = self.sources_containing(&key);
            if providers.is_empty() {
                continue;
            }

            let before_idx = self.current_winner_source_idx(vfs, &key, providers);
            let Some(after_idx) = self.winner_after_reorder(providers, &rank_by_source) else {
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
                    .filter(|key| path_glob_matches(bucket, &key_to_path_buf_lossy(key)))
                    .count(),
            })
            .collect();

        Ok(SimulationDelta {
            changed_winners: changed.len(),
            unchanged_winners: wins_after.iter().sum::<usize>() - changed.len(),
            by_source_gain_loss: rows,
            by_bucket: bucket_rows,
            changed_keys_sample: changed
                .into_iter()
                .take(opts.sample_limit)
                .map(|key| key_to_path_buf_lossy(&key))
                .collect(),
        })
    }

    pub(super) fn reordered_indices(&self, op: ReorderOp) -> io::Result<Vec<usize>> {
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

    pub(super) fn rank_by_source(&self, order: &[usize]) -> Vec<usize> {
        let mut ranks = vec![0usize; self.sources.len()];
        for (rank, src_idx) in order.iter().enumerate() {
            ranks[*src_idx] = rank;
        }
        ranks
    }

    pub(super) fn current_winner_source_idx(
        &self,
        vfs: &VFS,
        key: &NormalizedPath,
        providers: &[usize],
    ) -> Option<usize> {
        let winner = vfs.get_file(key)?;
        if winner.is_loose() {
            let normalized_path = normalize_host_path(winner.path());
            if let Some(idx) = providers.iter().copied().find(|idx| {
                self.sources[*idx].kind == SourceKind::LooseDir
                    && normalize_host_path(&self.provider_path(*idx, key)).as_ref()
                        == normalized_path.as_ref()
            }) {
                return Some(idx);
            }

            providers
                .iter()
                .copied()
                .filter(|idx| {
                    self.sources[*idx].kind == SourceKind::LooseDir
                        && winner.path().starts_with(&self.sources[*idx].path)
                })
                .max_by_key(|idx| self.sources[*idx].path.components().count())
        } else {
            let parent = winner.parent_archive_path()?;
            let parent = normalize_host_path(Path::new(&parent));
            providers.iter().copied().find(|idx| {
                self.sources[*idx].kind == SourceKind::Archive
                    && normalize_host_path(&self.sources[*idx].path).as_ref() == parent.as_ref()
            })
        }
    }

    pub(super) fn winner_after_reorder(
        &self,
        providers: &[usize],
        rank_by_source: &[usize],
    ) -> Option<usize> {
        providers
            .iter()
            .copied()
            .filter(|idx| self.sources[*idx].kind == SourceKind::LooseDir)
            .max_by_key(|idx| rank_by_source[*idx])
            .or_else(|| {
                providers
                    .iter()
                    .copied()
                    .max_by_key(|idx| rank_by_source[*idx])
            })
    }
}
