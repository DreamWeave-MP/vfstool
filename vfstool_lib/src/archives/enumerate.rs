// SPDX-License-Identifier: MIT OR Apache-2.0
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
            .filter_map(bethesda_entry_path_bytes)
            .filter_map(|path| normalized_archive_key(&path))
            .map(|key| crate::paths::key_to_path_buf(&key))
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
                .map(|key| crate::paths::key_to_path_buf(&key))
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
                    let path = bethesda_entry_path_bytes(entry)?;
                    normalized_archive_key(&path).map(|normalized| {
                        let vfs_file =
                            VfsFile::from_archive_bytes(&path, Arc::clone(stored_archive));
                        (normalized, vfs_file)
                    })
                })),
                #[cfg(feature = "zip")]
                TypedArchive::Zip(archive) => {
                    let entries: Vec<(NormalizedPath, VfsFile)> = if let Ok(mut guard) =
                        archive.lock()
                    {
                        (0..guard.len())
                            .filter_map(|zip_index| {
                                let Ok(entry) = guard.by_index(zip_index) else {
                                    return None;
                                };
                                if entry.is_dir() {
                                    return None;
                                }
                                let original_name = entry.name().to_string();
                                normalized_archive_key(original_name.as_bytes()).map(|normalized| {
                                    let vfs_file = VfsFile::from_zip_archive(
                                        &original_name,
                                        zip_index,
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

#[cfg(feature = "beth-archives")]
fn bethesda_entry_path_bytes(entry: dream_archive::Entry<'_>) -> Option<Vec<u8>> {
    match entry {
        dream_archive::Entry::Tes4Bsa(entry) => bethesda_path_bytes_from_parts(
            entry.path().map(|path| path.to_vec()),
            true,
            entry.folder().map(|folder| &**folder),
            entry.name().map(|name| &**name),
        ),
        _ => bethesda_path_bytes_from_parts(
            entry.path().map(|path| path.to_vec()),
            false,
            None,
            None,
        ),
    }
}

#[cfg(feature = "beth-archives")]
fn bethesda_path_bytes_from_parts(
    path: Option<Vec<u8>>,
    is_tes4: bool,
    folder: Option<&[u8]>,
    name: Option<&[u8]>,
) -> Option<Vec<u8>> {
    if path.is_some() || !is_tes4 {
        return path;
    }

    let folder = folder?;
    let name = name?;
    let mut reconstructed =
        Vec::with_capacity(folder.len() + usize::from(!folder.is_empty()) + name.len());
    reconstructed.extend_from_slice(folder);
    if !folder.is_empty() && !name.is_empty() {
        reconstructed.push(b'\\');
    }
    reconstructed.extend_from_slice(name);
    Some(reconstructed)
}

/// Build a normalized-path -> [`VfsFile`] map from an [`ArchiveList`].
///
/// Duplicate normalized keys within one archive list are collapsed by map insertion. Use
/// [`file_entries`] when provider/collision reporting needs to preserve every archive entry.
#[must_use]
pub fn file_map(archives: &ArchiveList) -> AHashMap<NormalizedPath, VfsFile> {
    file_entries(archives).into_iter().collect()
}

#[cfg(all(test, feature = "beth-archives"))]
mod tests {
    use super::*;
    use dream_archive::{Tes4BsaBuilder, bsa::tes4::ArchiveTypes};
    use std::{fs, path::Path};

    #[test]
    fn tes4_missing_facade_path_falls_back_to_folder_and_name() {
        let path =
            bethesda_path_bytes_from_parts(None, true, Some(b"meshes"), Some(b"test.nif")).unwrap();

        assert_eq!(path, b"meshes\\test.nif");
        assert_eq!(
            normalized_archive_key(&path).unwrap().as_bytes(),
            b"meshes/test.nif"
        );
    }

    #[test]
    fn non_tes4_entry_without_facade_path_is_not_reconstructed() {
        assert!(
            bethesda_path_bytes_from_parts(None, false, Some(b"meshes"), Some(b"test.nif"),)
                .is_none()
        );
    }

    #[test]
    fn facade_path_is_preferred_over_tes4_fallback_parts() {
        let path = bethesda_path_bytes_from_parts(
            Some(b"textures/example.dds".to_vec()),
            true,
            Some(b"meshes"),
            Some(b"test.nif"),
        )
        .unwrap();

        assert_eq!(path, b"textures/example.dds");
    }

    #[test]
    fn fallback_spelling_opens_real_fallout3_bsa_entry() {
        let dir = TempDir::new("beth_archive_fallback_open_path");
        let archive_path = dir.path().join("Fallout - Meshes.bsa");
        create_fallout3_bsa(&archive_path);

        let fallback_path =
            bethesda_path_bytes_from_parts(None, true, Some(b"meshes"), Some(b"test.nif")).unwrap();
        let archive = dream_archive::Archive::open_path(&archive_path).unwrap();

        assert_eq!(
            archive.read_file_required(&fallback_path).unwrap(),
            b"fallout3 mesh payload"
        );
    }

    fn create_fallout3_bsa(path: &Path) {
        let mut builder = Tes4BsaBuilder::fallout3();
        builder.set_archive_types(ArchiveTypes::MESHES);
        builder
            .add_bytes("meshes/test.nif", b"fallout3 mesh payload")
            .unwrap();
        builder.write_path(path).unwrap();
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "{name}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
