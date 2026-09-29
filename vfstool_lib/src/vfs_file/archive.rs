// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::archives::{StoredArchive, TypedArchive};

#[cfg(all(feature = "zip", not(test)))]
const MAX_BUFFERED_ZIP_ENTRY_SIZE: u64 = 512 * 1024 * 1024;

#[cfg(all(feature = "zip", test))]
const MAX_BUFFERED_ZIP_ENTRY_SIZE: u64 = 64;

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

    /// The entry's last path component. Archives separate folders with `\` (Morrowind's BSAs)
    /// or `/`, whatever the host's separator is, so `Path::file_name` on the whole entry path
    /// would keep a BSA entry's folders in its name on Linux and macOS.
    pub(super) fn last_component(&self) -> &Path {
        // `path` is built from a `str`, or from the entry's bytes read lossily, so it is UTF-8.
        let name = self.path.to_str().unwrap_or_default();
        Path::new(name.rsplit(['/', '\\']).next().unwrap_or(name))
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
                // The method first, from the raw reader, which never refuses an entry: the zip
                // crate decodes stored, deflate, and LZMA entries; Zstandard (method 93) goes
                // through ruzstd, a pure-Rust decoder, so no C library enters the build.
                let (name, size, method) = {
                    let raw = guard
                        .by_index_raw(zip_index)
                        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
                    (raw.name().to_owned(), raw.size(), raw.compression())
                };
                if size > MAX_BUFFERED_ZIP_ENTRY_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        format!(
                            "zip entry '{name}' is {size} bytes, exceeding the buffered entry limit of {MAX_BUFFERED_ZIP_ENTRY_SIZE} bytes"
                        ),
                    ));
                }
                let capacity = usize::try_from(size).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        format!("zip entry '{name}' is too large to buffer"),
                    )
                })?;
                let mut buf = Vec::with_capacity(capacity);
                // `ZSTD` is method 93 whether or not the zip crate was built to decode it.
                if method == zip::CompressionMethod::ZSTD {
                    let raw = guard
                        .by_index_raw(zip_index)
                        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
                    let decoder = ruzstd::decoding::StreamingDecoder::new(raw)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                    let mut limited_entry = decoder.take(MAX_BUFFERED_ZIP_ENTRY_SIZE + 1);
                    limited_entry.read_to_end(&mut buf)?;
                } else {
                    let entry = guard
                        .by_index(zip_index)
                        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
                    let mut limited_entry = entry.take(MAX_BUFFERED_ZIP_ENTRY_SIZE + 1);
                    limited_entry.read_to_end(&mut buf)?;
                }
                if u64::try_from(buf.len()).unwrap_or(u64::MAX) > MAX_BUFFERED_ZIP_ENTRY_SIZE {
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

/// The entry's uncompressed size when the archive's index records it: a TES3 BSA record, the
/// sum of a BA2 entry's chunk sizes, or a ZIP central directory entry. `None` when only
/// decompressing tells (a TES4 BSA stores the compressed size, and the true size inside the
/// data block), and for an entry the archive does not know.
pub(super) fn known_size(archive_ref: &ArchiveReference) -> io::Result<Option<u64>> {
    match archive_ref.parent_archive.handle() {
        #[cfg(feature = "beth-archives")]
        TypedArchive::Bethesda(archive) => Ok(match archive {
            dream_archive::Archive::Tes3Bsa(bsa) => bsa
                .get(&archive_ref.raw_path)
                .map(|entry| u64::from(entry.file().size)),
            dream_archive::Archive::BA2(ba2) => ba2.get(&archive_ref.raw_path).map(|entry| {
                entry
                    .file()
                    .chunks()
                    .iter()
                    .map(|chunk| u64::from(chunk.size()))
                    .sum()
            }),
            dream_archive::Archive::Tes4Bsa(_) => None,
        }),
        #[cfg(feature = "zip")]
        TypedArchive::Zip(archive) => {
            let Some(zip_index) = archive_ref.zip_index else {
                return Ok(None);
            };
            let mut guard = archive
                .lock()
                .map_err(|_| io::Error::other("zip mutex poisoned"))?;
            let raw = guard
                .by_index_raw(zip_index)
                .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
            Ok(Some(raw.size()))
        }
    }
}

/// Reads up to `dst.len()` bytes from `offset` of an entry stored without compression, straight
/// from the archive: a TES3 BSA entry, or a ZIP entry with the `Stored` method. `Ok(None)` for
/// any other entry, whose bytes only exist once the whole entry is decompressed. Returns the
/// number of bytes read, short only at the entry's end.
pub(super) fn read_stored_at(
    archive_ref: &ArchiveReference,
    offset: u64,
    dst: &mut [u8],
) -> io::Result<Option<usize>> {
    match archive_ref.parent_archive.handle() {
        #[cfg(feature = "beth-archives")]
        TypedArchive::Bethesda(archive) => {
            let dream_archive::Archive::Tes3Bsa(bsa) = archive else {
                return Ok(None);
            };
            let reader = bsa
                .open_file_required(&archive_ref.raw_path)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            read_from(reader, offset, dst).map(Some)
        }
        #[cfg(feature = "zip")]
        TypedArchive::Zip(archive) => {
            let Some(zip_index) = archive_ref.zip_index else {
                return Ok(None);
            };
            let mut guard = archive
                .lock()
                .map_err(|_| io::Error::other("zip mutex poisoned"))?;
            let raw = guard
                .by_index_raw(zip_index)
                .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e.to_string()))?;
            if raw.compression() != zip::CompressionMethod::Stored {
                return Ok(None);
            }
            read_from(raw, offset, dst).map(Some)
        }
    }
}

/// Skips `offset` bytes of `reader` and fills `dst` from there, stopping at the end.
fn read_from(mut reader: impl Read, offset: u64, dst: &mut [u8]) -> io::Result<usize> {
    io::copy(&mut reader.by_ref().take(offset), &mut io::sink())?;
    let mut filled = 0;
    while filled < dst.len() {
        match reader.read(&mut dst[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}
