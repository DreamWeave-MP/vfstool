// SPDX-License-Identifier: MIT OR Apache-2.0
#[cfg(feature = "zip")]
use super::keys::is_zip_or_pk3;
use super::{ArchiveList, StoredArchive, TypedArchive};
use crate::VfsFile;
use ahash::AHashMap;
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(feature = "bsa")]
use ba2::{self, prelude::*, tes3::Archive as TES3Archive};
#[cfg(feature = "zip")]
use std::sync::Mutex;

/// Open every archive named in `archive_list` that can be resolved through `file_map`.
#[must_use]
pub fn from_set(file_map: &AHashMap<PathBuf, VfsFile>, archive_list: &[&str]) -> ArchiveList {
    archive_list
        .iter()
        .copied()
        .filter_map(|archive| {
            let archive_path = crate::normalize_path(archive).into_owned();

            let Some(valid_archive) = file_map.get(&archive_path) else {
                eprintln!("vfstool: warning: archive '{archive}' not found in any data directory, skipping");
                return None;
            };

            open_archive(valid_archive.path())
        })
        .collect()
}

/// Try to open a single archive file, detecting its format by extension and content.
///
/// ZIP/PK3 files are identified by extension; BSA/BA2 files are identified by
/// magic bytes. Returns `None` with a warning on any failure.
#[allow(unreachable_code)]
pub(crate) fn open_archive(path: &Path) -> Option<Arc<StoredArchive>> {
    #[cfg(feature = "zip")]
    if is_zip_or_pk3(path) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to open zip '{}': {e}",
                    path.display()
                );
                return None;
            }
        };
        return match zip::ZipArchive::new(file) {
            Ok(archive) => Some(Arc::new(StoredArchive {
                file_handle: None,
                archive: TypedArchive::Zip(Mutex::new(archive)),
                path: path.to_path_buf(),
            })),
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to read zip '{}': {e}",
                    path.display()
                );
                None
            }
        };
    }

    #[cfg(feature = "bsa")]
    {
        let mut file_handle = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to open archive '{}': {e}",
                    path.display()
                );
                return None;
            }
        };
        let Some(format) = ba2::guess_format(&mut file_handle) else {
            eprintln!(
                "vfstool: warning: could not determine format of archive '{}', skipping",
                path.display()
            );
            return None;
        };
        return match format {
            ba2::FileFormat::TES3 => match TES3Archive::read(&file_handle) {
                Ok(archive) => Some(Arc::new(StoredArchive {
                    file_handle: Some(file_handle),
                    archive: TypedArchive::Tes3(archive),
                    path: path.to_path_buf(),
                })),
                Err(e) => {
                    eprintln!(
                        "vfstool: warning: failed to read TES3 archive '{}': {e}",
                        path.display()
                    );
                    None
                }
            },
            ba2::FileFormat::TES4 => match ba2::tes4::Archive::read(&file_handle) {
                Ok((archive, _meta)) => Some(Arc::new(StoredArchive {
                    file_handle: Some(file_handle),
                    archive: TypedArchive::Tes4(archive),
                    path: path.to_path_buf(),
                })),
                Err(e) => {
                    eprintln!(
                        "vfstool: warning: failed to read TES4 archive '{}': {e}",
                        path.display()
                    );
                    None
                }
            },
            ba2::FileFormat::FO4 => match ba2::fo4::Archive::read(&file_handle) {
                Ok((archive, _meta)) => Some(Arc::new(StoredArchive {
                    file_handle: Some(file_handle),
                    archive: TypedArchive::Fo4(archive),
                    path: path.to_path_buf(),
                })),
                Err(e) => {
                    eprintln!(
                        "vfstool: warning: failed to read FO4 archive '{}': {e}",
                        path.display()
                    );
                    None
                }
            },
        };
    }

    eprintln!(
        "vfstool: warning: '{}' is not a recognized archive format, skipping",
        path.display()
    );
    None
}
