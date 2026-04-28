// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
use crate::{
    NormalizedPath, SourceKind, SourceMeta, VfsFile, path_glob_matches,
    paths::key_to_path_buf_lossy,
};
use std::path::{Path, PathBuf};

impl VFS {
    /// Insert or replace a file at `key` in the resolved winner map.
    ///
    /// This is a winner-only mutation: replacing or removing a key does not reveal lower-priority
    /// providers that may have existed when the VFS was originally constructed.
    pub fn insert_file<P: crate::VfsKeyInput + ?Sized>(
        &mut self,
        key: &P,
        file: VfsFile,
    ) -> Option<VfsFile> {
        let normalized = key.to_safe_vfs_key()?;
        let (source, provider_path) = provider_source_and_path(&file);
        self.layer_index
            .set_single_provider(&normalized, source, provider_path);
        self.file_map.insert(normalized, file)
    }

    /// Insert or replace a loose file at `key` in the resolved winner map.
    pub fn insert_loose_file<K: crate::VfsKeyInput + ?Sized, P: AsRef<Path>>(
        &mut self,
        key: &K,
        physical_path: P,
    ) -> Option<VfsFile> {
        self.insert_file(key, VfsFile::from(physical_path))
    }

    /// Remove the current winner at `key` from this resolved VFS.
    ///
    /// This does not reveal any lower-priority provider; it removes the key from the materialized
    /// map entirely.
    pub fn remove_file<P: crate::VfsKeyInput + ?Sized>(&mut self, key: &P) -> Option<VfsFile> {
        let normalized = key.to_vfs_key();
        self.layer_index.remove_key(&normalized);
        self.file_map.remove(&normalized)
    }

    /// Remove every current winner whose normalized key starts with `prefix`.
    pub fn remove_prefix<P: crate::VfsKeyInput + ?Sized>(
        &mut self,
        prefix: &P,
    ) -> Vec<(NormalizedPath, VfsFile)> {
        let normalized = prefix.to_vfs_key();
        self.remove_matching(|key, _| key.as_bytes().starts_with(normalized.as_bytes()))
    }

    /// Remove every current winner accepted by `matcher`.
    pub fn remove_matching(
        &mut self,
        mut matcher: impl FnMut(&NormalizedPath, &VfsFile) -> bool,
    ) -> Vec<(NormalizedPath, VfsFile)> {
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
    pub fn remove_matching_glob(&mut self, glob: &str) -> Vec<(NormalizedPath, VfsFile)> {
        self.remove_matching(|key, _| path_glob_matches(glob, &key_to_path_buf_lossy(key)))
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
