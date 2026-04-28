// SPDX-License-Identifier: GPL-3.0-only
use std::{
    io::{self, Cursor, Read},
    path::PathBuf,
    sync::Arc,
};

use crate::archives::{StoredArchive, TypedArchive};

/// A reference to a single file within an open [`StoredArchive`].
#[derive(Debug, Clone)]
pub struct ArchiveReference {
    pub(super) path: PathBuf,
    pub(super) raw_path: Vec<u8>,
    pub(super) parent_archive: Arc<StoredArchive>,
}

impl ArchiveReference {
    pub(super) fn new(path: &str, parent_archive: Arc<StoredArchive>) -> Self {
        Self {
            path: PathBuf::from(path),
            raw_path: path.as_bytes().to_vec(),
            parent_archive,
        }
    }

    pub(super) fn from_bytes(path: &[u8], parent_archive: Arc<StoredArchive>) -> Self {
        let display_path = String::from_utf8_lossy(path).into_owned();
        Self {
            path: PathBuf::from(display_path),
            raw_path: path.to_vec(),
            parent_archive,
        }
    }
}

pub(super) fn open(archive_ref: &ArchiveReference) -> io::Result<Box<dyn Read + '_>> {
    let parent = archive_ref.parent_archive.handle();

    match parent {
        #[cfg(feature = "beth-archives")]
        TypedArchive::Bethesda(archive) => {
            let bytes = archive
                .read_file_required(&archive_ref.raw_path)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Box::new(Cursor::new(bytes)))
        }

        #[cfg(feature = "zip")]
        TypedArchive::Zip(archive) => {
            let path_string = archive_ref.path.to_string_lossy().to_string();
            // Deferred optimization: this shared ZipArchive lock serializes reads from the same
            // archive. If real-world extraction profiles show it matters, use per-worker archive
            // handles or another independent-entry reader design instead of splitting individual
            // compressed entries across threads (which is not the useful unit of parallelism here).
            let mut guard = archive
                .lock()
                .map_err(|_| io::Error::other("zip mutex poisoned"))?;
            let buf = {
                let mut entry = guard
                    .by_name(&path_string)
                    .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
                let mut buf = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or_default());
                io::copy(&mut entry, &mut buf)?;
                buf
            };
            Ok(Box::new(Cursor::new(buf)))
        }
    }
}
