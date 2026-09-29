// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{NormalizedPath, paths::normalized_safe_key_bytes};
#[cfg(feature = "zip")]
use std::path::Path;
/// Returns `true` if the path has a ZIP container extension (case-insensitive): `.zip`, or the
/// renamed-ZIP conventions `.pk3` and `.jpk`.
#[cfg(feature = "zip")]
pub(crate) fn is_zip_archive(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ZIP_EXTENSIONS
            .iter()
            .any(|known| e.eq_ignore_ascii_case(known))
    })
}

/// The extensions read as ZIP containers.
#[cfg(feature = "zip")]
pub(crate) const ZIP_EXTENSIONS: [&str; 3] = ["zip", "pk3", "jpk"];

pub(crate) fn normalized_archive_key(raw: &[u8]) -> Option<NormalizedPath> {
    normalized_safe_key_bytes(raw)
}
