// SPDX-License-Identifier: MIT OR Apache-2.0
//! Path normalization and safety helpers shared across VFS modules.

use dream_path::{NormalizedPath, is_normalized_path, normalize_path};
use std::{
    borrow::Cow,
    ffi::OsString,
    mem,
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

/// Normalize a host/source path by converting backslashes to forward slashes and lowercasing ASCII letters.
///
/// Returns a borrowed `Cow` when no transformation is needed, avoiding allocation on the fast path.
pub fn normalize_host_path<P: AsRef<Path> + ?Sized>(path: &P) -> Cow<'_, Path> {
    let p = path.as_ref();
    let bytes = p.as_os_str().as_encoded_bytes();
    if !bytes.iter().any(|&b| b == b'\\' || b.is_ascii_uppercase()) {
        return Cow::Borrowed(p);
    }
    let normalized: Vec<u8> = bytes
        .iter()
        .map(|&byte| match byte {
            b'\\' => b'/',
            b'A'..=b'Z' => byte + 32,
            _ => byte,
        })
        .collect();
    Cow::Owned(PathBuf::from(unsafe {
        OsString::from_encoded_bytes_unchecked(normalized)
    }))
}

/// Normalizes a host/source [`PathBuf`] in-place, reusing its heap allocation.
///
/// Converts backslashes to forward slashes and lowercases ASCII letters.
/// No-op if the path requires no changes.
pub fn normalize_host_path_in_place(path: &mut PathBuf) {
    if !path
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&b| b == b'\\' || b.is_ascii_uppercase())
    {
        return;
    }
    let mut bytes = mem::take(path).into_os_string().into_encoded_bytes();
    for byte in &mut bytes {
        match *byte {
            b'\\' => *byte = b'/',
            b'A'..=b'Z' => *byte += 32,
            _ => {}
        }
    }
    // SAFETY: We only modified ASCII bytes (\ -> / and A-Z -> a-z), which
    // preserves the encoding invariant on all platforms.
    *path = PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(bytes) });
}

pub(crate) fn normalized_safe_key(path: &Path) -> Option<NormalizedPath> {
    let normalized = normalize_host_path(path).into_owned();
    let normalized_text = normalized.to_string_lossy();
    if normalized_text.as_bytes().get(1) == Some(&b':') {
        return None;
    }

    let mut safe = PathBuf::new();
    for component in normalized.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    (!safe.as_os_str().is_empty()).then(|| NormalizedPath::new(safe.as_os_str().as_encoded_bytes()))
}

pub(crate) fn normalized_safe_key_bytes(path: &[u8]) -> Option<NormalizedPath> {
    if !normalized_safe_raw_bytes(path) {
        return None;
    }
    let normalized = NormalizedPath::new(path);
    normalized_safe_normalized_bytes(normalized.as_bytes()).then_some(normalized)
}

fn normalized_safe_raw_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty()
        || bytes.starts_with(b"/")
        || bytes.starts_with(b"\\")
        || bytes.get(1) == Some(&b':')
        || bytes.contains(&b'\0')
    {
        return false;
    }
    bytes
        .split(|&byte| byte == b'/' || byte == b'\\')
        .filter(|component| !component.is_empty())
        .all(|component| component != b"." && component != b"..")
}

pub(crate) fn normalized_safe_normalized_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty()
        || bytes.starts_with(b"/")
        || bytes.get(1) == Some(&b':')
        || bytes.contains(&b'\0')
    {
        return false;
    }
    bytes
        .split(|&byte| byte == b'/')
        .filter(|component| !component.is_empty())
        .all(|component| component != b"." && component != b"..")
}

/// Folds a search needle the way host paths are folded: `\` to `/` and ASCII lowercase, with
/// no separator collapsing, so a substring keeps its shape.
#[must_use]
pub(crate) fn fold_needle(needle: &[u8]) -> Cow<'_, [u8]> {
    if !needle.iter().any(|&b| b == b'\\' || b.is_ascii_uppercase()) {
        return Cow::Borrowed(needle);
    }
    Cow::Owned(
        needle
            .iter()
            .map(|&byte| match byte {
                b'\\' => b'/',
                b'A'..=b'Z' => byte + 32,
                _ => byte,
            })
            .collect(),
    )
}

#[must_use]
pub(crate) fn key_is_at_or_under_prefix(key: &[u8], mut prefix: &[u8]) -> bool {
    // A directory prefix may be written with trailing separators (`meshes/`); they name the
    // same directory as `meshes`. Normalization already turned `\` into `/`.
    while let Some(trimmed) = prefix.strip_suffix(b"/") {
        prefix = trimmed;
    }
    key == prefix
        || key
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with(b"/"))
}

/// A normalized key as a host [`PathBuf`]: byte for byte where the platform's paths carry
/// arbitrary bytes (Unix), and with replacement characters only where they cannot. Report
/// keys, provenance, lock manifests, and glob matching all go through this, so a key that is
/// not UTF-8 round-trips exactly instead of being looked up under a spelling nothing has.
#[must_use]
pub(crate) fn key_to_path_buf(key: &NormalizedPath) -> PathBuf {
    key_to_path_buf_bytes(key)
        .unwrap_or_else(|| PathBuf::from(String::from_utf8_lossy(key.as_bytes()).into_owned()))
}

#[must_use]
#[cfg(unix)]
pub(crate) fn key_to_path_buf_bytes(key: &NormalizedPath) -> Option<PathBuf> {
    if key.as_bytes().contains(&b'\0') {
        return None;
    }
    Some(key_to_path_buf_raw_bytes(key.as_bytes()))
}

#[must_use]
#[cfg(not(unix))]
pub(crate) fn key_to_path_buf_bytes(key: &NormalizedPath) -> Option<PathBuf> {
    key_to_path_buf_raw_bytes(key.as_bytes())
}

#[cfg(unix)]
fn key_to_path_buf_raw_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn key_to_path_buf_raw_bytes(bytes: &[u8]) -> Option<PathBuf> {
    String::from_utf8(bytes.to_vec()).ok().map(PathBuf::from)
}

#[must_use]
pub(crate) fn key_to_string_lossy(key: &NormalizedPath) -> String {
    String::from_utf8_lossy(key.as_bytes()).into_owned()
}

/// Whether `destination` already is the file at `source`: the same path, another spelling of it
/// on a case-insensitive file system, a hard link to it, or a symbolic link that resolves to it.
/// Replacing such a destination, or copying onto it, destroys the source it would be written from.
/// A destination that does not exist is not the source.
pub(crate) fn is_same_file(source: &Path, destination: &Path) -> std::io::Result<bool> {
    match same_file::is_same_file(source, destination) {
        Ok(same) => Ok(same),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// Serde helpers that write paths as text. JSON, YAML and TOML hold only UTF-8, and serde refuses
/// a `PathBuf` that is not, which failed a whole report over one file name. A path that is not
/// UTF-8 is written with U+FFFD in place of each invalid sequence, the spelling `NormalizedKey`
/// serializes with; the report in memory keeps the exact bytes.
#[cfg(feature = "serialize")]
pub(crate) mod lossy {
    use serde::Serializer;
    use std::path::{Path, PathBuf};

    pub(crate) fn path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&path.to_string_lossy())
    }

    pub(crate) fn paths<S: Serializer>(
        paths: &[PathBuf],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(paths.iter().map(|path| path.to_string_lossy()))
    }

    #[allow(clippy::ref_option)] // serde's serialize_with passes the field by reference
    pub(crate) fn optional_path<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match path {
            Some(path) => serializer.serialize_some(&path.to_string_lossy()),
            None => serializer.serialize_none(),
        }
    }

    #[allow(clippy::ref_option)] // serde's serialize_with passes the field by reference
    pub(crate) fn optional_paths<S: Serializer>(
        paths: &Option<Vec<PathBuf>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match paths {
            Some(paths) => serializer.serialize_some(
                &paths
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>(),
            ),
            None => serializer.serialize_none(),
        }
    }
}

mod sealed {
    use dream_path::NormalizedPath;
    use std::path::{Path, PathBuf};

    pub trait Sealed {}

    impl<T: Sealed + ?Sized> Sealed for &T {}
    impl Sealed for NormalizedPath {}
    impl Sealed for Path {}
    impl Sealed for PathBuf {}
    impl Sealed for str {}
    impl Sealed for String {}
    impl Sealed for [u8] {}
}

/// Input that can be normalized into a byte-first VFS key.
///
/// This trait is sealed; callers can pass the supported key-like types but cannot implement new
/// conversions outside this crate. Key normalization is part of the VFS contract, not a plugin slot.
pub trait VfsKeyInput: sealed::Sealed {
    /// Normalize this value into an owned VFS key.
    fn to_vfs_key(&self) -> NormalizedPath;

    /// The normalized key bytes, borrowed when this value already has the normalized spelling.
    ///
    /// Lookups use this so a key that is already normalized (a `NormalizedPath`, or a string a
    /// script or an index already spelled `textures/foo.dds`) costs no allocation.
    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.to_vfs_key().into())
    }

    /// Normalize this value into an owned VFS key if it is safe to materialize.
    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        let key = self.to_vfs_key();
        normalized_safe_normalized_bytes(key.as_bytes()).then_some(key)
    }
}

impl<T: VfsKeyInput + ?Sized> VfsKeyInput for &T {
    fn to_vfs_key(&self) -> NormalizedPath {
        (*self).to_vfs_key()
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        (*self).vfs_key_bytes()
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        (*self).to_safe_vfs_key()
    }
}

impl VfsKeyInput for NormalizedPath {
    fn to_vfs_key(&self) -> NormalizedPath {
        self.clone()
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.as_bytes())
    }
}

/// Borrows `bytes` when they already have the normalized spelling, normalizes them otherwise.
fn key_bytes_of(bytes: &[u8]) -> Cow<'_, [u8]> {
    if is_normalized_path(bytes) {
        Cow::Borrowed(bytes)
    } else {
        Cow::Owned(normalize_path(bytes))
    }
}

impl VfsKeyInput for Path {
    fn to_vfs_key(&self) -> NormalizedPath {
        NormalizedPath::new(self.as_os_str().as_encoded_bytes())
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        key_bytes_of(self.as_os_str().as_encoded_bytes())
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        normalized_safe_key(self)
    }
}

impl VfsKeyInput for PathBuf {
    fn to_vfs_key(&self) -> NormalizedPath {
        self.as_path().to_vfs_key()
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        self.as_path().vfs_key_bytes()
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        self.as_path().to_safe_vfs_key()
    }
}

impl VfsKeyInput for str {
    fn to_vfs_key(&self) -> NormalizedPath {
        NormalizedPath::new(self.as_bytes())
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        key_bytes_of(self.as_bytes())
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        normalized_safe_key_bytes(self.as_bytes())
    }
}

impl VfsKeyInput for String {
    fn to_vfs_key(&self) -> NormalizedPath {
        self.as_str().to_vfs_key()
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        self.as_str().vfs_key_bytes()
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        self.as_str().to_safe_vfs_key()
    }
}

/// Raw key bytes in any spelling: what a Luau string or an archive entry name is. No UTF-8 is
/// required anywhere in the VFS key contract.
impl VfsKeyInput for [u8] {
    fn to_vfs_key(&self) -> NormalizedPath {
        NormalizedPath::new(self)
    }

    fn vfs_key_bytes(&self) -> Cow<'_, [u8]> {
        key_bytes_of(self)
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        normalized_safe_key_bytes(self)
    }
}

#[cfg(test)]
#[path = "paths/tests.rs"]
mod tests;
