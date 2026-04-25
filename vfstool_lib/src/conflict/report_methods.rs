// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::reports::{
    ConflictSourceEntry, ConflictsReport, DiffReport, ShadowedReport, ShadowedSource, StatsReport,
    StatsRow, WhichResult,
};
use crate::vfs::VFS;
use crate::{normalize_path, normalize_path_in_place};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

impl ConflictIndex {
    fn source_idx_for_loose_file(&self, key: Option<&Path>, path: &Path) -> Option<usize> {
        if let Some(key) = key {
            let normalized_path = normalize_path(path);
            if let Some((idx, _)) = self.sources.iter().enumerate().find(|(_, src)| {
                normalize_path(&src.join(key)).as_ref() == normalized_path.as_ref()
            }) {
                return Some(idx);
            }
        }

        self.sources
            .iter()
            .enumerate()
            .filter(|(_, src)| path.starts_with(src))
            .max_by_key(|(_, src)| src.components().count())
            .map(|(idx, _)| idx)
    }

    fn source_idx_for_archive_path(&self, archive_path: &str) -> Option<usize> {
        let archive_path = normalize_path(Path::new(archive_path));
        self.sources
            .iter()
            .position(|src| normalize_path(src).as_ref() == archive_path.as_ref())
    }

    /// Build a [`ConflictsReport`] listing every source's overrides and overridden files.
    ///
    /// When `use_relative` is `true`, paths are relative VFS keys; otherwise
    /// they are joined with the source directory to form absolute paths.
    #[must_use]
    pub fn conflicts_report(&self, use_relative: bool) -> ConflictsReport {
        let sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, src)| {
                let resolve = |p: &PathBuf| -> PathBuf { report_path(src, p, use_relative) };
                let mut overrides: Vec<PathBuf> =
                    self.conflicts[i].overrides.iter().map(resolve).collect();
                let mut overridden_by: Vec<PathBuf> = self.conflicts[i]
                    .overridden_by
                    .iter()
                    .map(resolve)
                    .collect();
                overrides.sort();
                overridden_by.sort();
                ConflictSourceEntry {
                    path: src.clone(),
                    overrides,
                    overridden_by,
                }
            })
            .collect();
        ConflictsReport { sources }
    }

    /// Build a [`ShadowedReport`] listing sources whose files are entirely overridden.
    ///
    /// A source is "shadowed" when every file in the source is superseded by a
    /// higher-priority source.
    #[must_use]
    pub fn shadowed_report(&self, use_relative: bool) -> ShadowedReport {
        let sources = self
            .sources
            .iter()
            .enumerate()
            .filter_map(|(i, src)| {
                if self.source_file_counts[i] == 0
                    || self.conflicts[i].overridden_by.len() != self.source_file_counts[i]
                {
                    return None;
                }
                let resolve = |p: &PathBuf| -> PathBuf { report_path(src, p, use_relative) };
                let mut shadowed_files: Vec<PathBuf> = self.conflicts[i]
                    .overridden_by
                    .iter()
                    .map(resolve)
                    .collect();
                shadowed_files.sort();
                Some(ShadowedSource {
                    path: src.clone(),
                    shadowed_files,
                })
            })
            .collect();
        ShadowedReport { sources }
    }

    /// Determine which source wins for `path` and which others also contain it.
    ///
    /// Returns `None` if `path` is not in the VFS at all. When the file exists
    /// in only one source, `also_in` is empty and `is_unique` is `true`.
    #[must_use]
    pub fn which(&self, vfs: &VFS, path: &Path) -> Option<WhichResult> {
        let winner = vfs.get_file(path)?;

        let winner_display = if winner.is_loose() {
            winner.path().display().to_string()
        } else {
            winner.parent_archive_path().unwrap_or_default()
        };

        let mut normalized = path.to_path_buf();
        normalize_path_in_place(&mut normalized);
        let source_indices = self.sources_containing(&normalized);

        let winner_src_idx = if winner.is_loose() {
            self.source_idx_for_loose_file(Some(&normalized), winner.path())
        } else {
            winner
                .parent_archive_path()
                .and_then(|ap| self.source_idx_for_archive_path(&ap))
        };

        let also_in: Vec<PathBuf> = source_indices
            .iter()
            .filter(|&&idx| Some(idx) != winner_src_idx)
            .map(|&idx| self.sources[idx].clone())
            .collect();

        let is_unique = source_indices.is_empty();

        Some(WhichResult {
            winner: winner_display,
            also_in,
            is_unique,
        })
    }

    /// Compute per-source win/override/overridden counts.
    ///
    /// "Wins" is the number of VFS files served from that source (i.e., it has
    /// the highest priority for those files). "Overrides" and "overridden" come
    /// directly from the conflict sets.
    #[must_use]
    pub fn stats(&self, vfs: &VFS) -> StatsReport {
        let mut wins: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for (key, file) in vfs.iter() {
            let source_idx = if file.is_loose() {
                self.source_idx_for_loose_file(Some(key), file.path())
            } else {
                file.parent_archive_path()
                    .and_then(|ap| self.source_idx_for_archive_path(&ap))
            };
            if let Some(idx) = source_idx {
                *wins.entry(idx).or_insert(0) += 1;
            }
        }

        let rows = self
            .sources
            .iter()
            .enumerate()
            .map(|(i, src)| StatsRow {
                source: src.clone(),
                wins: wins.get(&i).copied().unwrap_or(0),
                overrides: self.conflicts[i].overrides.len(),
                overridden: self.conflicts[i].overridden_by.len(),
            })
            .collect();

        StatsReport { rows }
    }

    /// Compare two data directories: which files are shared, unique to each,
    /// and which has higher load-order priority.
    #[must_use]
    pub fn diff_report(&self, source_a: &Path, source_b: &Path) -> DiffReport {
        let vfs_a = VFS::from_directories([source_a], None);
        let vfs_b = VFS::from_directories([source_b], None);

        let keys_a: HashSet<PathBuf> = vfs_a.iter().map(|(k, _)| k.clone()).collect();
        let keys_b: HashSet<PathBuf> = vfs_b.iter().map(|(k, _)| k.clone()).collect();

        let mut shared: Vec<PathBuf> = keys_a.intersection(&keys_b).cloned().collect();
        let mut only_in_a: Vec<PathBuf> = keys_a.difference(&keys_b).cloned().collect();
        let mut only_in_b: Vec<PathBuf> = keys_b.difference(&keys_a).cloned().collect();
        shared.sort();
        only_in_a.sort();
        only_in_b.sort();

        let idx_a = self.sources.iter().position(|s| s == source_a);
        let idx_b = self.sources.iter().position(|s| s == source_b);
        let higher_priority = match (idx_a, idx_b) {
            (Some(a), Some(b)) => {
                if a > b {
                    source_a.to_path_buf()
                } else {
                    source_b.to_path_buf()
                }
            }
            _ => source_b.to_path_buf(),
        };

        DiffReport {
            source_a: source_a.to_path_buf(),
            source_b: source_b.to_path_buf(),
            higher_priority,
            shared,
            only_in_a,
            only_in_b,
        }
    }
}

fn report_path(source: &Path, key: &Path, use_relative: bool) -> PathBuf {
    if use_relative {
        return key.to_path_buf();
    }
    if is_archive_source(source) {
        return PathBuf::from(format!("{}::{}", source.display(), key.display()));
    }
    source.join(key)
}

fn is_archive_source(source: &Path) -> bool {
    source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "bsa" | "ba2" | "zip" | "pk3"
            )
        })
}
