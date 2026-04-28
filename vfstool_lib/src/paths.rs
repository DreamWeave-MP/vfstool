// SPDX-License-Identifier: GPL-3.0-only
//! Path normalization and safety helpers shared across VFS modules.

use dream_path::NormalizedPath;
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

#[must_use]
pub(crate) fn key_is_at_or_under_prefix(key: &NormalizedPath, prefix: &NormalizedPath) -> bool {
    let key = key.as_bytes();
    let prefix = prefix.as_bytes();
    key == prefix
        || key
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with(b"/"))
}

#[must_use]
pub(crate) fn key_to_path_buf_lossy(key: &NormalizedPath) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(key.as_bytes()).into_owned())
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
}

/// Input that can be normalized into a byte-first VFS key.
///
/// This trait is sealed; callers can pass the supported key-like types but cannot implement new
/// conversions outside this crate. Key normalization is part of the VFS contract, not a plugin slot.
pub trait VfsKeyInput: sealed::Sealed {
    /// Normalize this value into an owned VFS key.
    fn to_vfs_key(&self) -> NormalizedPath;

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

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        (*self).to_safe_vfs_key()
    }
}

impl VfsKeyInput for NormalizedPath {
    fn to_vfs_key(&self) -> NormalizedPath {
        self.clone()
    }
}

impl VfsKeyInput for Path {
    fn to_vfs_key(&self) -> NormalizedPath {
        NormalizedPath::new(self.as_os_str().as_encoded_bytes())
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        normalized_safe_key(self)
    }
}

impl VfsKeyInput for PathBuf {
    fn to_vfs_key(&self) -> NormalizedPath {
        self.as_path().to_vfs_key()
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        self.as_path().to_safe_vfs_key()
    }
}

impl VfsKeyInput for str {
    fn to_vfs_key(&self) -> NormalizedPath {
        NormalizedPath::new(self.as_bytes())
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        normalized_safe_key_bytes(self.as_bytes())
    }
}

impl VfsKeyInput for String {
    fn to_vfs_key(&self) -> NormalizedPath {
        self.as_str().to_vfs_key()
    }

    fn to_safe_vfs_key(&self) -> Option<NormalizedPath> {
        self.as_str().to_safe_vfs_key()
    }
}

#[cfg(test)]
#[path = "paths/tests.rs"]
mod tests;
