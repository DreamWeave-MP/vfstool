// SPDX-License-Identifier: GPL-3.0-only
use std::{
    io::{self, Read},
    path::PathBuf,
    sync::Arc,
};

use crate::archives::{StoredArchive, TypedArchive};

#[cfg(feature = "zip")]
const MAX_BUFFERED_ZIP_ENTRY_SIZE: u64 = 512 * 1024 * 1024;

/// A reference to a single file within an open [`StoredArchive`].
#[derive(Debug, Clone)]
pub struct ArchiveReference {
    pub(super) path: PathBuf,
    pub(super) raw_path: Vec<u8>,
    #[cfg(feature = "zip")]
    pub(super) zip_index: Option<usize>,
    pub(super) parent_archive: Arc<StoredArchive>,
}

impl ArchiveReference {
    pub(super) fn new(path: &str, parent_archive: Arc<StoredArchive>) -> Self {
        Self {
            path: PathBuf::from(path),
            raw_path: path.as_bytes().to_vec(),
            #[cfg(feature = "zip")]
            zip_index: None,
            parent_archive,
        }
    }

    #[cfg(feature = "zip")]
    pub(super) fn new_zip(
        path: &str,
        zip_index: usize,
        parent_archive: Arc<StoredArchive>,
    ) -> Self {
        Self {
            path: PathBuf::from(path),
            raw_path: path.as_bytes().to_vec(),
            zip_index: Some(zip_index),
            parent_archive,
        }
    }

    pub(super) fn from_bytes(path: &[u8], parent_archive: Arc<StoredArchive>) -> Self {
        let display_path = String::from_utf8_lossy(path).into_owned();
        Self {
            path: PathBuf::from(display_path),
            raw_path: path.to_vec(),
            #[cfg(feature = "zip")]
            zip_index: None,
            parent_archive,
        }
    }
}

pub(super) fn open(archive_ref: &ArchiveReference) -> io::Result<Box<dyn Read + '_>> {
    let parent = archive_ref.parent_archive.handle();

    match parent {
        #[cfg(feature = "beth-archives")]
        TypedArchive::Bethesda(archive) => {
            let reader = archive
                .open_file_required(&archive_ref.raw_path)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(reader)
        }

        #[cfg(feature = "zip")]
        TypedArchive::Zip(archive) => {
            // Deferred optimization: this shared ZipArchive lock serializes reads from the same
            // archive. If real-world extraction profiles show it matters, use per-worker archive
            // handles or another independent-entry reader design instead of splitting individual
            // compressed entries across threads (which is not the useful unit of parallelism here).
            let mut guard = archive
                .lock()
                .map_err(|_| io::Error::other("zip mutex poisoned"))?;
            let buf = {
                let Some(zip_index) = archive_ref.zip_index else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "zip archive reference is missing central-directory index",
                    ));
                };
                let mut entry = guard
                    .by_index(zip_index)
                    .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
                if entry.size() > MAX_BUFFERED_ZIP_ENTRY_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        format!(
                            "zip entry '{}' is {} bytes, exceeding the buffered entry limit of {} bytes",
                            entry.name(),
                            entry.size(),
                            MAX_BUFFERED_ZIP_ENTRY_SIZE
                        ),
                    ));
                }
                let capacity = usize::try_from(entry.size()).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        format!("zip entry '{}' is too large to buffer", entry.name()),
                    )
                })?;
                let mut buf = Vec::with_capacity(capacity);
                let copied = io::copy(&mut entry, &mut buf)?;
                if copied > MAX_BUFFERED_ZIP_ENTRY_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        format!(
                            "zip entry '{}' exceeded the buffered entry limit of {} bytes while reading",
                            archive_ref.path.display(),
                            MAX_BUFFERED_ZIP_ENTRY_SIZE
                        ),
                    ));
                }
                buf
            };
            Ok(Box::new(std::io::Cursor::new(buf)))
        }
    }
}
