// SPDX-License-Identifier: GPL-3.0-only
#[cfg(any(feature = "beth-archives", feature = "zip"))]
use std::sync::Arc;

use std::{
    fs::File as StdFile,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(any(feature = "beth-archives", feature = "zip"))]
use crate::archives::StoredArchive;

#[cfg(any(feature = "beth-archives", feature = "zip"))]
#[path = "vfs_file/archive.rs"]
mod archive;

#[cfg(any(feature = "beth-archives", feature = "zip"))]
pub use archive::ArchiveReference;

#[cfg(test)]
#[path = "vfs_file/tests.rs"]
mod tests;

/// Backing storage for a [`VfsFile`]: either a loose path on disk or an archive entry.
#[derive(Debug, Clone)]
pub enum FileType {
    /// File stored inside a BSA, BA2, ZIP, or PK3 archive.
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    Archive(ArchiveReference),
    /// Loose file on the real filesystem, identified by its absolute path.
    Loose(PathBuf),
}

/// Represents a file within the Virtual File System (VFS).
///
/// This struct encapsulates a file that exists in the real filesystem but is managed
/// within the VFS. Each `VfsFile` maintains a reference to its original, **non-normalized**
/// path to ensure correct file operations. Paths should only be normalized when **retrieved**,
/// not when constructing the file, as normalization may affect OS file resolution.
///
/// Files in the VFS should be **unique** and stored in a `HashMap` inside the `VFS` struct.
/// They are typically wrapped in `Arc<VfsFile>` for safe concurrent access.
#[derive(Debug, Clone)]
pub struct VfsFile {
    file: FileType,
}

impl Default for VfsFile {
    fn default() -> Self {
        Self {
            file: FileType::Loose(PathBuf::default()),
        }
    }
}

impl VfsFile {
    /// Creates a new `VfsFile` instance with the given `path`.
    ///
    /// # Arguments
    ///
    /// * `path` - An owned `PathBuf` representing the file's location on disk.
    ///
    /// # Notes
    ///
    /// - Paths **must not be normalized** at creation time to avoid potential file lookup issues.
    /// - `VfsFile` does not, itself, verify that the provided path exists at creation time;
    ///   this responsibility is left up to its constructor (typically, the VFS struct)
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool_lib::VfsFile;
    ///
    /// let path = "C:\\Morrowind\\Data Files\\Morrowind.esm";
    ///
    /// let file = VfsFile::from(path);
    /// assert_eq!(file.path().to_str(), Some(path));
    /// ```
    pub fn from<P: AsRef<Path>>(path: P) -> Self {
        VfsFile {
            file: FileType::Loose(path.as_ref().to_path_buf()),
        }
    }

    /// Creates a [`VfsFile`] backed by an entry inside `parent_archive`.
    ///
    /// `path` is the in-archive path of the file (not normalized; normalization happens at the
    /// VFS key level, not here).
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    pub fn from_archive<S: AsRef<str>>(path: S, parent_archive: Arc<StoredArchive>) -> Self {
        VfsFile {
            file: FileType::Archive(ArchiveReference::new(path.as_ref(), parent_archive)),
        }
    }

    /// Creates a [`VfsFile`] backed by a ZIP entry at a specific central-directory index.
    ///
    /// ZIP archives may contain exact duplicate entry names. The index is therefore part of the
    /// provider identity; opening by name would be a causality bug wearing a compression format.
    #[cfg(feature = "zip")]
    pub fn from_zip_archive<S: AsRef<str>>(
        path: S,
        zip_index: usize,
        parent_archive: Arc<StoredArchive>,
    ) -> Self {
        VfsFile {
            file: FileType::Archive(ArchiveReference::new_zip(
                path.as_ref(),
                zip_index,
                parent_archive,
            )),
        }
    }

    /// Creates a [`VfsFile`] backed by a byte-named entry inside `parent_archive`.
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    pub fn from_archive_bytes(path: &[u8], parent_archive: Arc<StoredArchive>) -> Self {
        VfsFile {
            file: FileType::Archive(ArchiveReference::from_bytes(path, parent_archive)),
        }
    }

    /// Returns `true` if this file is a loose file on the real filesystem.
    #[must_use]
    pub fn is_loose(&self) -> bool {
        match self.file {
            FileType::Loose(_) => true,
            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(_) => false,
        }
    }

    /// Returns `true` if this file is stored inside a BSA, BA2, ZIP, or PK3 archive.
    #[must_use]
    pub fn is_archive(&self) -> bool {
        match self.file {
            FileType::Loose(_) => false,
            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(_) => true,
        }
    }

    /// Returns the absolute path to the parent archive as a string, or `None` for loose files.
    #[must_use]
    pub fn parent_archive_path(&self) -> Option<String> {
        match &self.file {
            FileType::Loose(_) => None,
            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(archive_ref) => {
                let path_str = archive_ref
                    .parent_archive
                    .path()
                    .to_string_lossy()
                    .to_string();

                Some(path_str)
            }
        }
    }

    /// Returns just the file name of the parent archive (e.g. `"Morrowind.bsa"`), or `None` for loose files.
    #[must_use]
    pub fn parent_archive_name(&self) -> Option<String> {
        match &self.file {
            FileType::Loose(_) => None,

            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(archive_ref) => {
                let name = archive_ref
                    .parent_archive
                    .path()
                    .file_name()?
                    .to_string_lossy()
                    .to_string();

                Some(name)
            }
        }
    }

    /// Returns an `Arc` clone of the parent archive handle, or an error for loose files.
    #[cfg(any(feature = "beth-archives", feature = "zip"))]
    ///
    /// # Errors
    ///
    /// Returns an error when called on a loose-file-backed `VfsFile`.
    pub fn parent_archive_handle(&self) -> io::Result<Arc<StoredArchive>> {
        match &self.file {
            FileType::Loose(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Loose files may not return an archive reference!",
            )),
            FileType::Archive(archive_ref) => Ok(Arc::clone(&archive_ref.parent_archive)),
        }
    }

    /// Opens the file and returns a standard `File` handle.
    ///
    /// # Returns
    ///
    /// * `Ok(StdFile)` - If the file exists and can be opened.
    /// * `Err(io::Error)` - If the file does not exist or cannot be opened.
    ///
    /// # Errors
    ///
    /// Returns an error if opening or reading archive/loose file data fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool_lib::VfsFile;
    ///
    /// let path = "C:\\Some\\Very\\Long\\Path";
    ///
    /// let file = VfsFile::from(path);
    /// let result = file.open();
    ///
    /// assert!(result.is_err());
    /// ```
    pub fn open(&self) -> io::Result<Box<dyn Read + '_>> {
        match &self.file {
            FileType::Loose(path) => {
                let file = StdFile::open(path)?;
                Ok(Box::new(file))
            }

            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(archive_ref) => archive::open(archive_ref),
        }
    }

    /// Retrieves the file name (i.e., the last component of the path).
    ///
    /// # Returns
    ///
    /// * `Some(&str)` - If the path contains a valid file name.
    /// * `None` - If the path does not have a file name. This should be a rare exception as any
    ///   files typically used *will* have extensions, but it is not necessarily mandatory (eg unix
    ///   binaries)
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool_lib::VfsFile;
    ///
    /// let morrowind_esm = PathBuf::from("C:").join("Morrowind").join("Data
    /// Files").join("Morrowind.esm");
    ///
    /// let file = VfsFile::from(morrowind_esm);
    /// assert_eq!(file.file_name(), Some(std::ffi::OsStr::new("Morrowind.esm")));
    /// ```
    #[must_use]
    pub fn file_name(&self) -> Option<&std::ffi::OsStr> {
        match &self.file {
            FileType::Loose(path) => path.file_name(),
            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(archive_ref) => archive_ref.path.file_name(),
        }
    }

    ///
    /// Retrieves the file name (i.e., the last component of the path), without
    /// extensions.
    ///
    /// # Returns
    ///
    /// * `Some(&str)` - If the path contains a valid file name.
    /// * `None` - If the path does not have a file name. This should be a rare exception as any
    ///   files typically used *will* have extensions, but it is not necessarily mandatory (eg unix
    ///   binaries)
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool_lib::VfsFile;
    ///
    /// let morrowind_esm = PathBuf::from("C:").join("Morrowind").join("Data
    /// Files").join("Morrowind.esm");
    ///
    /// let file = VfsFile::from(morrowind_esm);
    /// assert_eq!(file.file_stem(), Some(std::ffi::OsStr::new("Morrowind")));
    /// ```
    #[must_use]
    pub fn file_stem(&self) -> Option<&std::ffi::OsStr> {
        match &self.file {
            FileType::Loose(path) => path.file_stem(),
            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(archive_ref) => archive_ref.path.file_stem(),
        }
    }

    /// Returns the original (non-normalized) path of the file.
    ///
    /// # Returns
    ///
    /// * `&Path` - The path used when creating this `VfsFile`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vfstool_lib::VfsFile;
    /// use std::path::PathBuf;
    ///
    /// let path = "C:\\Morrowind\\Data Files\\Morrowind.esm";
    ///
    /// let file = VfsFile::from(path);
    /// assert_eq!(file.path(), PathBuf::from(path));
    /// ```
    #[must_use]
    pub fn path(&self) -> &Path {
        match &self.file {
            FileType::Loose(path) => path.as_path(),

            #[cfg(any(feature = "beth-archives", feature = "zip"))]
            FileType::Archive(archive_ref) => &archive_ref.path,
        }
    }
}
