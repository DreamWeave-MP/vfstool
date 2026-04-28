// SPDX-License-Identifier: GPL-3.0-only
use crate::{NormalizedPath, paths::normalized_safe_key_bytes};
#[cfg(feature = "zip")]
use std::path::Path;
/// Returns `true` if the path has a ZIP or PK3 extension (case-insensitive).
#[cfg(feature = "zip")]
pub(crate) fn is_zip_or_pk3(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("pk3"))
}

pub(crate) fn normalized_archive_key(raw: &[u8]) -> Option<NormalizedPath> {
    normalized_safe_key_bytes(raw)
}
