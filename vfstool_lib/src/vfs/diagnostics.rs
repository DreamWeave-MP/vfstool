// SPDX-License-Identifier: GPL-3.0-only
use std::{error::Error, fmt, io, path::PathBuf};

/// Error returned by strict VFS construction APIs.
///
/// The legacy `from_directories*` constructors are intentionally best-effort for compatibility:
/// unreadable paths and archives that fail to open are skipped. The `try_from_directories*`
/// constructors return this error instead, because silently producing a partial VFS is a very tidy
/// way to make every later report lie.
#[derive(Debug)]
#[non_exhaustive]
pub enum VfsBuildError {
    /// Walking a loose data directory failed.
    Traversal {
        /// Directory or entry that failed during traversal.
        path: PathBuf,
        /// Original I/O error reported by `walkdir`.
        source: io::Error,
    },
    /// A configured archive was not present in the scanned loose data directories.
    ArchiveNotFound {
        /// Archive name as supplied by the caller.
        archive: String,
    },
    /// A configured archive was found but could not be opened.
    ArchiveLoad {
        /// Resolved archive path.
        archive: PathBuf,
        /// Loader diagnostic. This may come from ZIP, Bethesda archive loading, or unsupported
        /// archive format detection depending on enabled features.
        message: String,
    },
}

impl fmt::Display for VfsBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Traversal { path, source } => {
                write!(f, "failed to traverse {}: {source}", path.display())
            }
            Self::ArchiveNotFound { archive } => {
                write!(f, "configured archive not found: {archive}")
            }
            Self::ArchiveLoad { archive, message } => {
                write!(f, "failed to load archive {}: {message}", archive.display())
            }
        }
    }
}

impl Error for VfsBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Traversal { source, .. } => Some(source),
            Self::ArchiveNotFound { .. } | Self::ArchiveLoad { .. } => None,
        }
    }
}
