// SPDX-License-Identifier: MIT OR Apache-2.0
#[cfg(feature = "bsa")]
use ba2::{
    fo4::{ArchiveKey as Fo4ArchiveKey, File as Fo4File},
    tes3::ArchiveKey as Tes3Key,
    tes4::{
        ArchiveKey as Tes4ArchiveKey, DirectoryKey as Tes4DirKey, File as Tes4File,
        FileCompressionOptions as Tes4CompressionOptions,
    },
};

use std::{
    io::{self, Cursor, Read},
    path::PathBuf,
    sync::Arc,
};

#[cfg(feature = "bsa")]
use std::{borrow::Cow, path::Path};

use crate::archives::{StoredArchive, TypedArchive};

/// Streaming reader over a Fallout 4 BA2 file stored as multiple chunks.
#[cfg(feature = "bsa")]
pub struct Fo4FileReader<'a> {
    chunks: std::vec::IntoIter<&'a [u8]>,
    current_chunk: Option<&'a [u8]>,
    position: usize,
}

#[cfg(feature = "bsa")]
/// Since FO4 Archives are stored in chunks, implement a custom reader for them
/// This allows to seamlessly call read on them as we do for other all other file types
impl<'a> Fo4FileReader<'a> {
    /// Creates a [`Fo4FileReader`] that streams chunks from `file` in order.
    #[must_use]
    pub fn new(file: &'a Fo4File) -> Self {
        let mut chunks = file
            .iter()
            .map(ba2::fo4::Chunk::as_bytes)
            .collect::<Vec<_>>()
            .into_iter();
        let current_chunk = chunks.next();

        Self {
            chunks,
            current_chunk,
            position: 0,
        }
    }
}

#[cfg(feature = "bsa")]
impl Read for Fo4FileReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut total_read = 0;

        while total_read < buf.len() {
            let chunk = match self.current_chunk {
                Some(chunk) if self.position < chunk.len() => chunk,
                _ => {
                    // Move to the next chunk
                    self.current_chunk = self.chunks.next();
                    self.position = 0;
                    match self.current_chunk {
                        Some(chunk) => chunk,
                        None => return Ok(total_read), // No more data
                    }
                }
            };

            let remaining = chunk.len() - self.position;
            let to_read = (buf.len() - total_read).min(remaining);

            buf[total_read..total_read + to_read]
                .copy_from_slice(&chunk[self.position..self.position + to_read]);

            self.position += to_read;
            total_read += to_read;
        }

        Ok(total_read)
    }
}

/// Reader over a TES4 (Oblivion/Skyrim BSA) file, decompressing on construction if needed.
#[cfg(feature = "bsa")]
pub struct TES4FileReader<'a> {
    data: Cursor<Cow<'a, [u8]>>, // Borrow raw files; own decompressed data only when required.
}

#[cfg(feature = "bsa")]
impl<'a> TES4FileReader<'a> {
    /// Creates a new `TES4FileReader` for a TES4 file.
    ///
    /// If the file is compressed, it will be decompressed before being wrapped in the reader.
    ///
    /// # Errors
    ///
    /// Returns an error if decompression fails.
    pub fn new(file: &'a Tes4File) -> io::Result<Self> {
        let data = if file.is_compressed() {
            Cow::Owned(
                file.decompress(&Tes4CompressionOptions::default())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
                    .as_bytes()
                    .to_vec(),
            )
        } else {
            Cow::Borrowed(file.as_bytes())
        };

        Ok(Self {
            data: Cursor::new(data),
        })
    }
}

#[cfg(feature = "bsa")]
impl Read for TES4FileReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.data.read(buf)
    }
}

/// A reference to a single file within an open [`StoredArchive`].
#[derive(Debug, Clone)]
pub struct ArchiveReference {
    pub(super) path: PathBuf,
    pub(super) parent_archive: Arc<StoredArchive>,
}

impl ArchiveReference {
    pub(super) fn new(path: &str, parent_archive: Arc<StoredArchive>) -> Self {
        Self {
            path: PathBuf::from(path),
            parent_archive,
        }
    }
}

#[cfg(feature = "bsa")]
impl ArchiveReference {
    /// Decompose a normalized VFS path into a TES4 directory key and file key pair.
    ///
    /// # Errors
    ///
    /// Returns an error if the path has no parent directory or file name.
    pub fn tes4_keys(path: &Path) -> io::Result<(Tes4ArchiveKey<'_>, Tes4DirKey<'_>)> {
        let path = path.to_string_lossy();
        let Some((dir, file)) = path.rsplit_once(['/', '\\']) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Missing parent directory in TES4 archive",
            ));
        };
        if dir.is_empty() || file.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Missing TES4 archive directory or file name",
            ));
        }

        let dir_key: Tes4ArchiveKey = dir.to_owned().into();
        let file_key: Tes4DirKey = file.to_owned().into();

        Ok((dir_key, file_key))
    }
}

pub(super) fn open(archive_ref: &ArchiveReference) -> io::Result<Box<dyn Read + '_>> {
    let parent = archive_ref.parent_archive.handle();
    let path_string = archive_ref.path.to_string_lossy().to_string();

    match parent {
        #[cfg(feature = "bsa")]
        TypedArchive::Tes3(archive) => {
            let key: Tes3Key = path_string.into();
            let file = archive.get(&key).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "File not found in TES3 archive")
            })?;
            Ok(Box::new(Cursor::new(file.as_bytes())))
        }

        #[cfg(feature = "bsa")]
        TypedArchive::Tes4(archive) => {
            let (dir_key, file_key) = ArchiveReference::tes4_keys(archive_ref.path.as_path())?;
            let file: &Tes4File = archive
                .get(&dir_key)
                .and_then(|dir| dir.get(&file_key))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "File not found in TES4 archive")
                })?;
            Ok(Box::new(TES4FileReader::new(file)?))
        }

        #[cfg(feature = "bsa")]
        TypedArchive::Fo4(archive) => {
            let key: Fo4ArchiveKey = path_string.into();
            let file: &Fo4File = archive.get(&key).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "File not found in FO4 archive")
            })?;
            Ok(Box::new(Fo4FileReader::new(file)))
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
