// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

/// Record the mtime (nanoseconds since epoch) and byte length of every file
/// under `dir`. Returns a map from relative path to `(mtime_ns, len)`.
pub fn snapshot_directory(dir: &Path) -> io::Result<HashMap<PathBuf, (u64, u64)>> {
    let mut map = HashMap::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let meta = entry.metadata()?;
            let rel = entry
                .path()
                .strip_prefix(dir)
                .expect("walkdir entry should be under root")
                .to_path_buf();
            let mtime = meta
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            map.insert(rel, (mtime, meta.len()));
        }
    }
    Ok(map)
}

/// Walk `dir` and return the relative paths of every file that either was not
/// present in `baseline` or whose mtime / size differs from the baseline entry.
pub fn changed_files(
    dir: &Path,
    baseline: &HashMap<PathBuf, (u64, u64)>,
) -> io::Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dir)
            .expect("walkdir entry should be under root")
            .to_path_buf();
        let meta = entry.metadata()?;
        let mtime = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let is_changed = match baseline.get(&rel) {
            None => true,
            Some(&(m, l)) => mtime != m || meta.len() != l,
        };
        if is_changed {
            changed.push(rel);
        }
    }
    Ok(changed)
}
