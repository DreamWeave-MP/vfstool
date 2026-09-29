// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::{
    SourceKind, SourceMeta, VfsKeyInput,
    reports::{ConflictSourceEntry, ConflictsReport, ShadowedReport, ShadowedSource},
};
use std::path::{Path, PathBuf};

impl ConflictIndex {
    /// Build a [`ConflictsReport`] listing every source's overrides and overridden files.
    ///
    /// When `use_relative` is `true`, paths are relative VFS keys; otherwise a loose source's are
    /// its path joined with each file's own spelling inside it, and an archive's `ARCHIVE::KEY`.
    #[must_use]
    pub fn conflicts_report(&self, use_relative: bool) -> ConflictsReport {
        let sources = self
            .source_meta
            .iter()
            .enumerate()
            .map(|(i, source)| {
                let resolve = |key: &PathBuf| self.report_path(i, source, key, use_relative);
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
        self.shadowed_report_with_files(use_relative, true)
    }

    /// Build a [`ShadowedReport`] with optional per-file listings.
    ///
    /// A source is still considered shadowed using the full provider index. When `list_files` is
    /// false, each returned [`ShadowedSource`] has an empty `shadowed_files` list so summary reports
    /// do not serialize every overridden key.
    #[must_use]
    pub fn shadowed_report_with_files(
        &self,
        use_relative: bool,
        list_files: bool,
    ) -> ShadowedReport {
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
                let shadowed_files = if list_files {
                    let resolve = |key: &PathBuf| self.report_path(i, source, key, use_relative);
                    let mut shadowed_files: Vec<PathBuf> = self.conflicts[i]
                        .overridden_by
                        .iter()
                        .map(resolve)
                        .collect();
                    shadowed_files.sort();
                    shadowed_files
                } else {
                    Vec::new()
                };
                Some(ShadowedSource {
                    path: source.path.clone(),
                    shadowed_files,
                })
            })
            .collect();
        ShadowedReport { sources }
    }
}

impl ConflictIndex {
    /// `key` as a report shows it: the key itself when `use_relative`; for an archive,
    /// `ARCHIVE::key`; for a loose source, the file's path on disk, joined from the source path
    /// and the file's own spelling inside it, which on a case-sensitive file system is the only
    /// one that exists.
    fn report_path(
        &self,
        source_index: usize,
        source: &SourceMeta,
        key: &Path,
        use_relative: bool,
    ) -> PathBuf {
        if use_relative {
            return key.to_path_buf();
        }
        if source.kind == SourceKind::Archive {
            return PathBuf::from(format!("{}::{}", source.path.display(), key.display()));
        }
        let original = self
            .original_paths
            .get(&(source_index, key.to_vfs_key()))
            .map_or(key, PathBuf::as_path);
        source.path.join(original)
    }
}
