// SPDX-License-Identifier: GPL-3.0-only
use super::{ArchiveList, StoredArchive, TypedArchive, keys::normalized_archive_key};
use crate::VfsFile;
use ahash::AHashMap;
use std::{path::PathBuf, sync::Arc};

/// Return the normalized VFS paths for all files in an already-open archive.
#[must_use]
pub fn archive_paths(stored: &StoredArchive) -> Vec<PathBuf> {
    match &stored.archive {
        #[cfg(feature = "bsa")]
        TypedArchive::Tes3(data) => data
            .iter()
            .filter_map(|(key, _)| normalized_archive_key(&key.name().to_string()))
            .collect(),
        #[cfg(feature = "bsa")]
        TypedArchive::Tes4(data) => data
            .iter()
            .flat_map(|(dir_key, dir)| {
                let dir_str = dir_key.name().to_string();
                dir.iter()
                    .filter_map(move |(key, _)| {
                        normalized_archive_key(&format!("{}\\{}", dir_str, key.name()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
        #[cfg(feature = "bsa")]
        TypedArchive::Fo4(data) => data
            .iter()
            .filter_map(|(key, _)| normalized_archive_key(&key.name().to_string()))
            .collect(),
        #[cfg(feature = "zip")]
        TypedArchive::Zip(archive) => {
            let Ok(guard) = archive.lock() else {
                return Vec::new();
            };
            guard
                .file_names()
                .filter(|name| !name.ends_with('/'))
                .filter_map(normalized_archive_key)
                .collect()
        }
    }
}

/// Build a normalized-path -> [`VfsFile`] map from an [`ArchiveList`].
#[must_use]
pub fn file_map(archives: &ArchiveList) -> AHashMap<PathBuf, VfsFile> {
    archives
        .iter()
        .flat_map(|stored_archive| {
            let iter: Box<dyn Iterator<Item = (PathBuf, VfsFile)>> = match &stored_archive.archive {
                #[cfg(feature = "bsa")]
                TypedArchive::Tes3(data) => Box::new(data.iter().filter_map(|(key, _value)| {
                    let name_string = key.name().to_string();
                    normalized_archive_key(&name_string).map(|normalized| {
                        (
                            normalized,
                            VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                        )
                    })
                })),
                #[cfg(feature = "bsa")]
                TypedArchive::Tes4(data) => {
                    Box::new(data.iter().flat_map(move |(dir_key, dir)| {
                        let dir_string = dir_key.name();
                        dir.iter().filter_map(move |(key, _value)| {
                            let archive_path = format!("{}\\{}", dir_string, key.name());
                            normalized_archive_key(&archive_path).map(|normalized| {
                                let vfs_file = VfsFile::from_archive(
                                    &archive_path,
                                    Arc::clone(stored_archive),
                                );
                                (normalized, vfs_file)
                            })
                        })
                    }))
                }
                #[cfg(feature = "bsa")]
                TypedArchive::Fo4(data) => Box::new(data.iter().filter_map(|(key, _value)| {
                    let name_string = key.name().to_string();
                    normalized_archive_key(&name_string).map(|normalized| {
                        (
                            normalized,
                            VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                        )
                    })
                })),
                #[cfg(feature = "zip")]
                TypedArchive::Zip(archive) => {
                    let entries: Vec<(PathBuf, VfsFile)> = if let Ok(guard) = archive.lock() {
                        guard
                            .file_names()
                            .filter(|name| !name.ends_with('/'))
                            .filter_map(|name| {
                                let original_name = name.to_string();
                                normalized_archive_key(name).map(|normalized| {
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
