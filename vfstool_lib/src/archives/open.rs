// SPDX-License-Identifier: GPL-3.0-only
#[cfg(feature = "zip")]
use super::keys::is_zip_or_pk3;
use super::{ArchiveList, StoredArchive, TypedArchive};
use crate::{NormalizedPath, VfsFile};
use ahash::AHashMap;
#[cfg(feature = "zip")]
use std::fs::File;
use std::{path::Path, sync::Arc};

#[cfg(feature = "zip")]
use std::sync::Mutex;

/// Open every archive named in `archive_list` that can be resolved through `file_map`.
#[must_use]
pub fn from_set(
    file_map: &AHashMap<NormalizedPath, VfsFile>,
    archive_list: &[&str],
) -> ArchiveList {
    archive_list
        .iter()
        .copied()
        .filter_map(|archive| {
            let archive_path = NormalizedPath::new(archive.as_bytes());

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
        return match dream_archive::Archive::open_path(path) {
            Ok(archive) => Some(Arc::new(StoredArchive {
                file_handle: None,
                archive: TypedArchive::Bethesda(archive),
                path: path.to_path_buf(),
            })),
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to read Bethesda archive '{}': {e}",
                    path.display()
                );
                None
            }
        };
    }

    eprintln!(
        "vfstool: warning: '{}' is not a recognized archive format, skipping",
        path.display()
    );
    None
}
