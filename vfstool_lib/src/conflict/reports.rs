// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::reports::{ConflictSourceEntry, ConflictsReport, ShadowedReport, ShadowedSource};
use std::path::{Path, PathBuf};

impl ConflictIndex {
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
