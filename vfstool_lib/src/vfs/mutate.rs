// SPDX-License-Identifier: MIT OR Apache-2.0
use super::VFS;
use crate::{
    SourceKind, SourceMeta, VfsFile, normalize_path, path_glob_matches, paths::normalized_safe_key,
};
use std::path::{Path, PathBuf};

impl VFS {
    /// Insert or replace a file at `key` in the resolved winner map.
    ///
    /// This is a winner-only mutation: replacing or removing a key does not reveal lower-priority
    /// providers that may have existed when the VFS was originally constructed.
    pub fn insert_file<P: AsRef<Path>>(&mut self, key: P, file: VfsFile) -> Option<VfsFile> {
        let normalized = normalized_safe_key(key.as_ref())?;
        let (source, provider_path) = provider_source_and_path(&file);
        self.layer_index
            .set_single_provider(&normalized, source, provider_path);
        self.file_map.insert(normalized, file)
    }

    /// Insert or replace a loose file at `key` in the resolved winner map.
    pub fn insert_loose_file<K: AsRef<Path>, P: AsRef<Path>>(
        &mut self,
        key: K,
        physical_path: P,
    ) -> Option<VfsFile> {
        self.insert_file(key, VfsFile::from(physical_path))
    }

    /// Remove the current winner at `key` from this resolved VFS.
    ///
    /// This does not reveal any lower-priority provider; it removes the key from the materialized
    /// map entirely.
    pub fn remove_file<P: AsRef<Path>>(&mut self, key: P) -> Option<VfsFile> {
        let normalized = normalize_path(key.as_ref()).into_owned();
        self.layer_index.remove_key(&normalized);
        self.file_map.remove(&normalized)
    }

    /// Remove every current winner whose normalized key starts with `prefix`.
    pub fn remove_prefix<P: AsRef<Path>>(&mut self, prefix: P) -> Vec<(PathBuf, VfsFile)> {
        let normalized = normalize_path(prefix.as_ref()).into_owned();
        self.remove_matching(|key, _| key.starts_with(&normalized))
    }

    /// Remove every current winner accepted by `matcher`.
    pub fn remove_matching(
        &mut self,
        mut matcher: impl FnMut(&Path, &VfsFile) -> bool,
    ) -> Vec<(PathBuf, VfsFile)> {
        let keys = self
            .file_map
            .iter()
            .filter_map(|(key, file)| matcher(key, file).then_some(key.clone()))
            .collect::<Vec<_>>();

        keys.into_iter()
            .filter_map(|key| {
                self.layer_index.remove_key(&key);
                self.file_map.remove(&key).map(|file| (key, file))
            })
            .collect()
    }

    /// Remove every current winner whose normalized key matches `glob`.
    pub fn remove_matching_glob(&mut self, glob: &str) -> Vec<(PathBuf, VfsFile)> {
        self.remove_matching(|key, _| path_glob_matches(glob, key))
    }
}

fn provider_source_and_path(file: &VfsFile) -> (SourceMeta, PathBuf) {
    if file.is_archive() {
        (
            SourceMeta {
                path: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
                kind: SourceKind::Archive,
            },
            file.path().to_path_buf(),
        )
    } else {
        let source_path = file
            .path()
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        let provider_path = file
            .path()
            .strip_prefix(&source_path)
            .map_or_else(|_| file.path().to_path_buf(), Path::to_path_buf);
        (
            SourceMeta {
                path: source_path,
                kind: SourceKind::LooseDir,
            },
            provider_path,
        )
    }
}
