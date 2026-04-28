// SPDX-License-Identifier: GPL-3.0-only
use super::{MaybeFile, VFS, VFSTuple};
use crate::{DisplayTree, NormalizedPath, VfsKeyInput, normalize_path, paths::key_to_string_lossy};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

impl VFS {
    /// Looks up a file in the VFS after normalizing the path.
    ///
    /// Already-normalized paths skip the allocation — the fast path is a
    /// direct `&Path` lookup with no heap activity.
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> MaybeFile<'_> {
        let key = NormalizedPath::new(path.as_ref().as_os_str().as_encoded_bytes());
        self.file_map.get(&key)
    }

    /// Search the VFS using a case-insensitive regex pattern.
    ///
    /// Returns a filtered [`DisplayTree`] containing only files whose VFS path
    /// matches `pattern`. The pattern is compiled with `case_insensitive(true)`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `pattern` is not a valid regex.
    pub fn find_by_regex(
        &self,
        pattern: &str,
        relative: bool,
    ) -> std::result::Result<DisplayTree, regex::Error> {
        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()?;
        Ok(self.tree_filtered(relative, |key, _file| {
            re.is_match(&key_to_string_lossy(key))
        }))
    }

    /// Return a filtered tree showing files from or replacing `filter_path`.
    ///
    /// `all_dirs` is the full ordered list of data directories (as from openmw.cfg).
    /// The method builds a single-directory VFS for `filter_path`, then filters
    /// the full VFS accordingly.
    ///
    /// When `replacements_only` is `false`: files still served from `filter_path`.
    /// When `replacements_only` is `true`: files where `filter_path` has a copy
    /// but the full VFS serves them from a different (higher-priority) source.
    #[must_use]
    pub fn remaining(
        &self,
        filter_path: &Path,
        replacements_only: bool,
        all_dirs: &[PathBuf],
        relative: bool,
    ) -> DisplayTree {
        let filter_normalized = normalize_path(filter_path).into_owned();

        let filtered_dirs: Vec<&PathBuf> = all_dirs
            .iter()
            .filter(|d| normalize_path(d.as_path()) == filter_normalized.as_path())
            .collect();

        let filtered_vfs = VFS::from_directories(filtered_dirs, None);

        self.tree_filtered(relative, |key, file| {
            if replacements_only {
                filtered_vfs.contains(key)
                    && !normalize_path(file.path()).starts_with(&filter_normalized)
            } else {
                normalize_path(file.path()).starts_with(&filter_normalized)
            }
        })
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl Iterator<Item = VFSTuple<'_>> {
        let needle = Self::normalize_substring(substring);
        self.file_map.iter().filter_map(move |(path, file)| {
            key_to_string_lossy(path)
                .contains(&needle)
                .then_some((path, file))
        })
    }

    /// Given a substring, return a parallel iterator over all paths that contain it.
    pub fn par_paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let needle = Self::normalize_substring(substring);
        self.file_map.par_iter().filter_map(move |(path, file)| {
            key_to_string_lossy(path)
                .contains(&needle)
                .then_some((path, file))
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn paths_with<P: AsRef<Path>>(&self, prefix: P) -> impl Iterator<Item = VFSTuple<'_>> {
        let normalized_prefix = NormalizedPath::new(prefix.as_ref().as_os_str().as_encoded_bytes());
        self.file_map.iter().filter_map(move |(path, file)| {
            path.as_bytes()
                .starts_with(normalized_prefix.as_bytes())
                .then_some((path, file))
        })
    }

    /// Given a path prefix to a location in the VFS, return a parallel iterator to *all* of its contents.
    pub fn par_paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let normalized_prefix = NormalizedPath::new(prefix.as_ref().as_os_str().as_encoded_bytes());
        self.file_map.par_iter().filter_map(move |(path, file)| {
            path.as_bytes()
                .starts_with(normalized_prefix.as_bytes())
                .then_some((path, file))
        })
    }

    fn normalize_substring<S: AsRef<str>>(s: S) -> String {
        normalize_path(s.as_ref()).to_string_lossy().into_owned()
    }

    /// Returns `true` if the VFS contains a file at `key`.
    ///
    /// `key` is a normalized relative VFS path (e.g. `"textures/foo.dds"`).
    /// The path is normalized before lookup, so case and separator variants
    /// are accepted. Already-normalized keys skip the allocation.
    #[must_use]
    pub fn contains<K: VfsKeyInput + ?Sized>(&self, key: &K) -> bool {
        let key = key.to_vfs_key();
        self.file_map.contains_key(&key)
    }
}
