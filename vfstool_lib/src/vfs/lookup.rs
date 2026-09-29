// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{MaybeFile, VFS, VFSTuple};
use crate::{
    DisplayTree, VfsKeyInput, normalize_host_path,
    paths::{fold_needle, key_is_at_or_under_prefix, key_to_string_lossy},
};
use dream_path::{bstr::ByteSlice as _, is_normalized_path};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

impl VFS {
    /// Looks up a file in the VFS after normalizing the key.
    ///
    /// Already-normalized keys skip path conversion and use their byte key
    /// directly; host paths and strings are normalized before lookup.
    pub fn get_file<P: VfsKeyInput + ?Sized>(&self, path: &P) -> MaybeFile<'_> {
        self.file_map.get(path.vfs_key_bytes().as_ref())
    }

    /// Looks up a file by a key that already has the normalized spelling
    /// ([`dream_path::is_normalized_path`]), with no normalization and no allocation.
    ///
    /// This is the lookup a caller uses after normalizing into its own scratch buffer
    /// ([`dream_path::normalize_path_into`]), so a hot loop over unnormalized input allocates
    /// nothing. A key that is not normalized is simply not found.
    #[must_use]
    pub fn get_file_normalized(&self, key: &[u8]) -> MaybeFile<'_> {
        debug_assert!(
            is_normalized_path(key),
            "get_file_normalized takes a normalized key"
        );
        self.file_map.get(key)
    }

    /// Number of resolved keys in the VFS.
    #[must_use]
    pub fn len(&self) -> usize {
        self.file_map.len()
    }

    /// Whether the VFS resolves no keys at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.file_map.is_empty()
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
        let filter_normalized = normalize_host_path(filter_path).into_owned();

        let filtered_dirs: Vec<&PathBuf> = all_dirs
            .iter()
            .filter(|d| normalize_host_path(d.as_path()) == filter_normalized.as_path())
            .collect();

        let filtered_vfs = VFS::from_directories(filtered_dirs, None);

        self.tree_filtered(relative, |key, file| {
            let file_path = normalize_host_path(file.path()).into_owned();
            if replacements_only {
                filtered_vfs.contains(key) && !path_is_at_or_under(&file_path, &filter_normalized)
            } else {
                path_is_at_or_under(&file_path, &filter_normalized)
            }
        })
    }

    /// Given a substring, return an iterator over all paths that contain it.
    ///
    /// The substring is folded like a host path (`\` to `/`, ASCII lowercase) and matched
    /// against the normalized key bytes.
    pub fn paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl Iterator<Item = VFSTuple<'_>> {
        let needle = fold_needle(substring.as_ref().as_bytes()).into_owned();
        self.file_map
            .iter()
            .filter(move |(path, _)| path.as_bytes().contains_str(&needle))
    }

    /// [`VFS::paths_matching`] over raw bytes: the needle may be any byte string, and keys are
    /// compared byte for byte, so a key that is not UTF-8 matches exactly.
    pub fn paths_matching_bytes(&self, substring: &[u8]) -> impl Iterator<Item = VFSTuple<'_>> {
        let needle = fold_needle(substring).into_owned();
        self.file_map
            .iter()
            .filter(move |(path, _)| path.as_bytes().contains_str(&needle))
    }

    /// Given a substring, return a parallel iterator over all paths that contain it.
    pub fn par_paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let needle = fold_needle(substring.as_ref().as_bytes()).into_owned();
        self.file_map
            .par_iter()
            .filter(move |(path, _)| path.as_bytes().contains_str(&needle))
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    ///
    /// The prefix is normalized like any VFS key and matched on path component boundaries;
    /// trailing separators are ignored, so `meshes`, `meshes/`, and `Meshes\` are equivalent.
    pub fn paths_with<P: VfsKeyInput + ?Sized>(
        &self,
        prefix: &P,
    ) -> impl Iterator<Item = VFSTuple<'_>> {
        let normalized_prefix = prefix.vfs_key_bytes().into_owned();
        self.file_map
            .iter()
            .filter(move |(path, _)| key_is_at_or_under_prefix(path.as_bytes(), &normalized_prefix))
    }

    /// Given a path prefix to a location in the VFS, return a parallel iterator to *all* of its contents.
    ///
    /// Accepts the same prefix spellings as [`VFS::paths_with`].
    pub fn par_paths_with<P: VfsKeyInput + ?Sized>(
        &self,
        prefix: &P,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let normalized_prefix = prefix.vfs_key_bytes().into_owned();
        self.file_map
            .par_iter()
            .filter(move |(path, _)| key_is_at_or_under_prefix(path.as_bytes(), &normalized_prefix))
    }

    /// Returns `true` if the VFS contains a file at `key`.
    ///
    /// `key` is a normalized relative VFS path (e.g. `"textures/foo.dds"`).
    /// The path is normalized before lookup, so case and separator variants
    /// are accepted. Already-normalized keys skip the allocation.
    #[must_use]
    pub fn contains<K: VfsKeyInput + ?Sized>(&self, key: &K) -> bool {
        self.file_map.contains_key(key.vfs_key_bytes().as_ref())
    }

    /// [`VFS::contains`] for a key that already has the normalized spelling; see
    /// [`VFS::get_file_normalized`].
    #[must_use]
    pub fn contains_normalized(&self, key: &[u8]) -> bool {
        debug_assert!(
            is_normalized_path(key),
            "contains_normalized takes a normalized key"
        );
        self.file_map.contains_key(key)
    }
}

fn path_is_at_or_under(path: &Path, root: &Path) -> bool {
    path == root || path.strip_prefix(root).is_ok()
}
