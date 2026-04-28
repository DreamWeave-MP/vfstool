// SPDX-License-Identifier: GPL-3.0-only
use std::{
    fmt,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(feature = "zip")]
use std::sync::Mutex;

/// An open archive, tagged by its format.
pub enum TypedArchive {
    /// Bethesda BSA/BA2 archive.
    #[cfg(feature = "beth-archives")]
    Bethesda(dream_archive::Archive),
    /// ZIP or PK3 archive.
    #[cfg(feature = "zip")]
    Zip(Mutex<zip::ZipArchive<File>>),
}

impl fmt::Debug for TypedArchive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "beth-archives")]
            Self::Bethesda(_) => f.write_str("TypedArchive::Bethesda"),
            #[cfg(feature = "zip")]
            Self::Zip(_) => f.write_str("TypedArchive::Zip"),
        }
    }
}

/// An opened, heap-allocated archive together with its on-disk path.
#[derive(Debug)]
pub struct StoredArchive {
    /// Reserved for archive readers that require an external file handle.
    #[allow(dead_code)]
    pub(super) file_handle: Option<File>,
    pub(super) archive: TypedArchive,
    pub(super) path: PathBuf,
}

impl StoredArchive {
    /// Returns the typed archive handle.
    #[must_use]
    pub fn handle(&self) -> &TypedArchive {
        &self.archive
    }

    /// Returns the stored path to the archive file on disk.
    ///
    /// This is the path the archive was opened with; it is not canonicalized here.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Ordered list of open archive handles.
pub type ArchiveList = Vec<Arc<StoredArchive>>;
