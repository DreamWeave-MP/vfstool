// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::normalize_path;
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Stable identifier for a source in load-order position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SourceId(usize);

impl SourceId {
    /// Construct a source ID from its load-order index.
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }

    /// Return the underlying load-order index.
    #[must_use]
    pub const fn as_index(self) -> usize {
        self.0
    }
}

/// Canonical normalized VFS key wrapper.
///
/// This type guarantees lowercase ASCII and forward slash separators.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct NormalizedKey(PathBuf);

impl NormalizedKey {
    /// Create a normalized key from any path-like value.
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self(normalize_path(path.as_ref()).into_owned())
    }

    /// Borrow as a [`Path`].
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Convert to an owned [`PathBuf`].
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl From<PathBuf> for NormalizedKey {
    fn from(value: PathBuf) -> Self {
        Self::new(value)
    }
}

impl From<&Path> for NormalizedKey {
    fn from(value: &Path) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for NormalizedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Content digest metadata used by higher-level analyses.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ContentDigest {
    /// Digest algorithm name.
    pub algorithm: &'static str,
    /// Lowercase hex digest string.
    pub hex: String,
    /// Content size in bytes.
    pub size: u64,
}

impl ContentDigest {
    /// Construct a BLAKE3 digest from bytes and size.
    #[must_use]
    pub fn blake3(bytes: [u8; 32], size: u64) -> Self {
        use std::fmt::Write;

        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(&mut hex, "{byte:02x}");
        }
        Self {
            algorithm: "blake3",
            hex,
            size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_key_normalizes_case_and_separators() {
        let key = NormalizedKey::new(PathBuf::from("Textures\\FOO.DDS"));
        assert_eq!(key.as_path(), Path::new("textures/foo.dds"));
    }

    #[test]
    fn source_id_round_trip_index() {
        let id = SourceId::from_index(7);
        assert_eq!(id.as_index(), 7);
    }

    #[test]
    fn content_digest_blake3_has_expected_shape() {
        let digest = ContentDigest::blake3([0xAB; 32], 123);
        assert_eq!(digest.algorithm, "blake3");
        assert_eq!(digest.hex.len(), 64);
        assert_eq!(digest.size, 123);
    }
}
