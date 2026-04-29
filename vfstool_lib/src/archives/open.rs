// SPDX-License-Identifier: GPL-3.0-only
#[cfg(feature = "zip")]
use super::keys::is_zip_or_pk3;
use super::{ArchiveList, StoredArchive, TypedArchive};
use crate::{NormalizedPath, VfsFile};
use ahash::AHashMap;
use rayon::prelude::*;
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
        .par_iter()
        .map(|archive| {
            let archive_path = NormalizedPath::new(archive.as_bytes());
            file_map
                .get(&archive_path)
                .and_then(|valid_archive| open_archive(valid_archive.path()))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect()
}

/// Try to open a single archive file, detecting its format by extension and content.
///
/// ZIP/PK3 files are identified by extension; BSA/BA2 files are identified by
/// magic bytes. Returns `None` on any failure.
#[allow(unreachable_code)]
pub(crate) fn open_archive(path: &Path) -> Option<Arc<StoredArchive>> {
    try_open_archive(path).ok()
}

#[allow(unreachable_code)]
pub(crate) fn try_open_archive(path: &Path) -> Result<Arc<StoredArchive>, String> {
    #[cfg(feature = "zip")]
    if is_zip_or_pk3(path) {
        let file = File::open(path).map_err(|err| err.to_string())?;
        return match zip::ZipArchive::new(file) {
            Ok(archive) => Ok(Arc::new(StoredArchive {
                file_handle: None,
                archive: TypedArchive::Zip(Mutex::new(archive)),
                path: path.to_path_buf(),
            })),
            Err(err) => Err(err.to_string()),
        };
    }

    #[cfg(feature = "beth-archives")]
    {
        return match dream_archive::Archive::open_path(path) {
            Ok(archive) => Ok(Arc::new(StoredArchive {
                file_handle: None,
                archive: TypedArchive::Bethesda(archive),
                path: path.to_path_buf(),
            })),
            Err(err) => Err(err.to_string()),
        };
    }

    Err(format!(
        "unsupported archive type or archive feature not enabled: {}",
        path.display()
    ))
}
