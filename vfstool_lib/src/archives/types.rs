// SPDX-License-Identifier: MIT OR Apache-2.0
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
    /// TES3 (Morrowind) BSA archive.
    #[cfg(feature = "bsa")]
    Tes3(ba2::tes3::Archive<'static>),
    /// TES4 (Oblivion/Skyrim) BSA archive.
    #[cfg(feature = "bsa")]
    Tes4(ba2::tes4::Archive<'static>),
    /// Fallout 4 BA2 archive.
    #[cfg(feature = "bsa")]
    Fo4(ba2::fo4::Archive<'static>),
    /// ZIP or PK3 archive.
    #[cfg(feature = "zip")]
    Zip(Mutex<zip::ZipArchive<File>>),
}

impl fmt::Debug for TypedArchive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "bsa")]
            Self::Tes3(_) => f.write_str("TypedArchive::Tes3"),
            #[cfg(feature = "bsa")]
            Self::Tes4(_) => f.write_str("TypedArchive::Tes4"),
            #[cfg(feature = "bsa")]
            Self::Fo4(_) => f.write_str("TypedArchive::Fo4"),
            #[cfg(feature = "zip")]
            Self::Zip(_) => f.write_str("TypedArchive::Zip"),
        }
    }
}

/// An opened, heap-allocated archive together with its on-disk path.
#[derive(Debug)]
pub struct StoredArchive {
    /// Keeps the BSA/BA2 memory-map file handle alive.
    /// `None` for ZIP/PK3 archives because `ZipArchive` owns its file handle internally.
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

    /// Returns the absolute path to the archive file on disk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Ordered list of open archive handles.
pub type ArchiveList = Vec<Arc<StoredArchive>>;
