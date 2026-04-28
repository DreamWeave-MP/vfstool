// SPDX-License-Identifier: GPL-3.0-only
use super::{ArchiveList, StoredArchive, TypedArchive, keys::normalized_archive_key};
use crate::{NormalizedPath, VfsFile};
use ahash::AHashMap;
use std::{path::PathBuf, sync::Arc};

/// Return the normalized VFS paths for all files in an already-open archive.
#[must_use]
pub fn archive_paths(stored: &StoredArchive) -> Vec<PathBuf> {
    match &stored.archive {
        #[cfg(feature = "beth-archives")]
        TypedArchive::Bethesda(data) => data
            .entries()
            .filter_map(dream_archive::Entry::path)
            .filter_map(|path| normalized_archive_key(path.as_ref()))
            .map(|key| crate::paths::key_to_path_buf_lossy(&key))
            .collect(),
        #[cfg(feature = "zip")]
        TypedArchive::Zip(archive) => {
            let Ok(guard) = archive.lock() else {
                return Vec::new();
            };
            guard
                .file_names()
                .filter(|name| !name.ends_with('/'))
                .filter_map(|name| normalized_archive_key(name.as_bytes()))
                .map(|key| crate::paths::key_to_path_buf_lossy(&key))
                .collect()
        }
    }
}

/// Build normalized archive entries from an [`ArchiveList`], preserving duplicate normalized keys.
#[must_use]
pub fn file_entries(archives: &ArchiveList) -> Vec<(NormalizedPath, VfsFile)> {
    archives
        .iter()
        .flat_map(|stored_archive| {
            let iter: Box<dyn Iterator<Item = (NormalizedPath, VfsFile)>> = match &stored_archive
                .archive
            {
                #[cfg(feature = "beth-archives")]
                TypedArchive::Bethesda(data) => Box::new(data.entries().filter_map(|entry| {
                    let path = entry.path()?;
                    normalized_archive_key(path.as_ref()).map(|normalized| {
                        let vfs_file =
                            VfsFile::from_archive_bytes(path.as_ref(), Arc::clone(stored_archive));
                        (normalized, vfs_file)
                    })
                })),
                #[cfg(feature = "zip")]
                TypedArchive::Zip(archive) => {
                    let entries: Vec<(NormalizedPath, VfsFile)> = if let Ok(guard) = archive.lock()
                    {
                        guard
                            .file_names()
                            .filter(|name| !name.ends_with('/'))
                            .filter_map(|name| {
                                let original_name = name.to_string();
                                normalized_archive_key(name.as_bytes()).map(|normalized| {
                                    let vfs_file = VfsFile::from_archive(
                                        &original_name,
                                        Arc::clone(stored_archive),
                                    );
                                    (normalized, vfs_file)
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    Box::new(entries.into_iter())
                }
            };
            iter
        })
        .collect()
}

/// Build a normalized-path -> [`VfsFile`] map from an [`ArchiveList`].
///
/// Duplicate normalized keys within one archive list are collapsed by map insertion. Use
/// [`file_entries`] when provider/collision reporting needs to preserve every archive entry.
#[must_use]
pub fn file_map(archives: &ArchiveList) -> AHashMap<NormalizedPath, VfsFile> {
    file_entries(archives).into_iter().collect()
}
