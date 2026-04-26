// SPDX-License-Identifier: GPL-3.0-only
use super::ConflictIndex;
use crate::{
    SourceKind, SourceMeta,
    reports::{ConflictSourceEntry, ConflictsReport, ShadowedReport, ShadowedSource},
};
use std::path::{Path, PathBuf};

impl ConflictIndex {
    /// Build a [`ConflictsReport`] listing every source's overrides and overridden files.
    ///
    /// When `use_relative` is `true`, paths are relative VFS keys; otherwise
    /// they are joined with the source directory to form absolute paths.
    #[must_use]
    pub fn conflicts_report(&self, use_relative: bool) -> ConflictsReport {
        let sources = self
            .source_meta
            .iter()
            .enumerate()
            .map(|(i, source)| {
                let resolve = |p: &PathBuf| -> PathBuf { report_path(source, p, use_relative) };
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
                    path: source.path.clone(),
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
            .source_meta
            .iter()
            .enumerate()
            .filter_map(|(i, source)| {
                if self.source_file_counts[i] == 0
                    || self.conflicts[i].overridden_by.len() != self.source_file_counts[i]
                {
                    return None;
                }
                let resolve = |p: &PathBuf| -> PathBuf { report_path(source, p, use_relative) };
                let mut shadowed_files: Vec<PathBuf> = self.conflicts[i]
                    .overridden_by
                    .iter()
                    .map(resolve)
                    .collect();
                shadowed_files.sort();
                Some(ShadowedSource {
                    path: source.path.clone(),
                    shadowed_files,
                })
            })
            .collect();
        ShadowedReport { sources }
    }
}

fn report_path(source: &SourceMeta, key: &Path, use_relative: bool) -> PathBuf {
    if use_relative {
        return key.to_path_buf();
    }
    if source.kind == SourceKind::Archive {
        return PathBuf::from(format!("{}::{}", source.path.display(), key.display()));
    }
    source.path.join(key)
}
