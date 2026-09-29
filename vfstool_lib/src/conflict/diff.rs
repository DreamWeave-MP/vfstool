// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::{VFS, normalize_host_path, paths::key_to_string_lossy, reports::DiffReport};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

impl ConflictIndex {
    /// Compare two sources: which keys they share, which only one of them provides, and which has
    /// higher load-order priority.
    ///
    /// Each path names the sources in [`ConflictIndex::sources`] spelled exactly like it or, when
    /// none is, those equal to it once ASCII case and separators are folded, the way a file system
    /// that ignores case finds them. Their keys come from this index, so an archive source and an
    /// index built from file lists compare by what the index holds. A path that names no source is
    /// read from disk as one data directory, loose files only.
    ///
    /// `source_a` and `source_b` in the report are the index's spelling of each source, or the
    /// path as given for one it does not have. `higher_priority` is whichever of the two comes
    /// later in `sources`; when either is not in it, it is `source_b`.
    #[must_use]
    pub fn diff_report(&self, source_a: &Path, source_b: &Path) -> DiffReport {
        let positions_a = self.source_positions(source_a);
        let positions_b = self.source_positions(source_b);
        let keys_a = self.source_keys(&positions_a, source_a);
        let keys_b = self.source_keys(&positions_b, source_b);

        let mut shared: Vec<String> = keys_a.intersection(&keys_b).cloned().collect();
        let mut only_in_a: Vec<String> = keys_a.difference(&keys_b).cloned().collect();
        let mut only_in_b: Vec<String> = keys_b.difference(&keys_a).cloned().collect();
        shared.sort();
        only_in_a.sort();
        only_in_b.sort();

        let source_a = self.source_spelling(&positions_a, source_a);
        let source_b = self.source_spelling(&positions_b, source_b);
        let higher_priority = match (positions_a.iter().max(), positions_b.iter().max()) {
            (Some(a), Some(b)) if a > b => source_a.clone(),
            _ => source_b.clone(),
        };

        DiffReport {
            source_a,
            source_b,
            higher_priority,
            shared,
            only_in_a,
            only_in_b,
        }
    }

    /// Positions in `sources` that `path` names: its exact spelling, or failing that, its spelling
    /// with ASCII case and separators folded.
    fn source_positions(&self, path: &Path) -> Vec<usize> {
        let exact: Vec<usize> = (0..self.sources.len())
            .filter(|&index| self.sources[index] == path)
            .collect();
        if !exact.is_empty() {
            return exact;
        }
        let folded = normalize_host_path(path);
        (0..self.sources.len())
            .filter(|&index| normalize_host_path(&self.sources[index]) == folded)
            .collect()
    }

    fn source_keys(&self, positions: &[usize], path: &Path) -> HashSet<String> {
        if positions.is_empty() {
            return VFS::from_directories([path], None)
                .iter()
                .map(|(key, _)| key_to_string_lossy(key))
                .collect();
        }
        self.path_to_sources
            .iter()
            .filter(|(_, sources)| sources.iter().any(|source| positions.contains(source)))
            .map(|(key, _)| key_to_string_lossy(key))
            .collect()
    }

    fn source_spelling(&self, positions: &[usize], path: &Path) -> PathBuf {
        positions
            .iter()
            .max()
            .map_or_else(|| path.to_path_buf(), |&index| self.sources[index].clone())
    }
}
