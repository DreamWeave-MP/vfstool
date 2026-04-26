// SPDX-License-Identifier: GPL-3.0-only
//! Path normalization and safety helpers shared across VFS modules.

use std::{
    borrow::Cow,
    ffi::OsString,
    mem,
    path::{Component, Path, PathBuf},
};

/// Normalize a path by converting backslashes to forward slashes and lowercasing ASCII letters.
///
/// Returns a borrowed `Cow` when no transformation is needed, avoiding allocation on the fast path.
pub fn normalize_path<P: AsRef<Path> + ?Sized>(path: &P) -> Cow<'_, Path> {
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

/// Normalizes a [`PathBuf`] in-place, reusing its heap allocation.
///
/// Converts backslashes to forward slashes and lowercases ASCII letters.
/// No-op if the path requires no changes.
pub fn normalize_path_in_place(path: &mut PathBuf) {
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

pub(crate) fn normalized_safe_key(path: &Path) -> Option<PathBuf> {
    let normalized = normalize_path(path).into_owned();
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

    (!safe.as_os_str().is_empty()).then_some(safe)
}

#[cfg(test)]
#[path = "paths/tests.rs"]
mod tests;
