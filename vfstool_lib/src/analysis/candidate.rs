// SPDX-License-Identifier: GPL-3.0-only
use super::{
    CandidateConflict, CandidatePlan, CandidatePlanOpts, CandidatePlanSummary, LayerIndex,
};
use crate::{NormalizedPath, VFS};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

use super::provider_io::hash_reader;

impl LayerIndex {
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

        let additions_by_key = diff
            .additions
            .into_iter()
            .map(|(key, _incoming)| key)
            .collect::<BTreeSet<_>>();
        let additions = additions_by_key.iter().cloned().collect::<Vec<_>>();

        let mut conflicts_by_key = BTreeMap::new();
        for (key, incoming, existing) in diff.conflicts {
            conflicts_by_key.entry(key).or_insert((incoming, existing));
        }

        let mut conflicts = Vec::new();
        let mut displaced_winners = Vec::new();

        for (key, (incoming, existing)) in conflicts_by_key {
            let normalized_key = NormalizedPath::new(key.as_os_str().as_encoded_bytes());
            let providers = self.sources_containing(&normalized_key);
            let current_winner_source = self
                .current_winner_source_idx(vfs, &normalized_key, providers)
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
}
