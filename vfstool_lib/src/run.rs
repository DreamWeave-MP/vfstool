// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::vfs::VFS;
use std::{
    collections::HashMap,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};
use rayon::prelude::*;
use walkdir::WalkDir;

pub type Snapshot = HashMap<PathBuf, [u8; 32]>;

/// Dump the VFS to `merged_dir` and capture a baseline snapshot.
///
/// Returns the number of files dumped and the snapshot for use with
/// [`run_finalize`]. The caller is responsible for executing the subprocess
/// between these two calls.
pub fn run_setup(vfs: &VFS, merged_dir: &Path, use_hardlinks: bool) -> io::Result<(usize, Snapshot)> {
    std::fs::create_dir_all(merged_dir)?;
    let count = vfs.dump_to_directory(merged_dir, use_hardlinks)?;
    let baseline = snapshot_directory(merged_dir)?;
    Ok((count, baseline))
}

/// Copy files changed since `baseline` from `merged_dir` into `output_dir`.
///
/// Returns a list of `(relative_path, destination_path)` pairs for every file
/// that was copied. The caller should call this only after the subprocess
/// succeeds.
pub fn run_finalize(
    merged_dir: &Path,
    baseline: &Snapshot,
    output_dir: &Path,
) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let changed = changed_files(merged_dir, baseline)?;
    let mut copied = Vec::new();

    for rel in changed {
        let dest = output_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(merged_dir.join(&rel), &dest)?;
        copied.push((rel, dest));
    }

    Ok(copied)
}

/// Compute the BLAKE3 hash of a file's contents.
/// Uses a 64 KiB stack buffer — avoids loading the whole file into memory.
pub fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Hash every file under `dir` in parallel.
/// Returns a map from relative path to its BLAKE3 digest.
pub fn snapshot_directory(dir: &Path) -> io::Result<HashMap<PathBuf, [u8; 32]>> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .par_bridge()
        .map(|entry| {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .expect("walkdir entry should be under root")
                .to_path_buf();
            let hash = hash_file(entry.path())?;
            Ok((rel, hash))
        })
        .collect::<io::Result<HashMap<_, _>>>()
}

/// Walk `dir` and return relative paths of files whose content differs from `baseline`.
/// Files in `baseline` that no longer exist in `dir` are silently ignored.
pub fn changed_files(
    dir: &Path,
    baseline: &HashMap<PathBuf, [u8; 32]>,
) -> io::Result<Vec<PathBuf>> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .par_bridge()
        .filter_map(|entry| {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .expect("walkdir entry should be under root")
                .to_path_buf();
            let hash = match hash_file(entry.path()) {
                Ok(h) => h,
                Err(e) => return Some(Err(e)),
            };
            let is_changed = match baseline.get(&rel) {
                None => true,
                Some(bh) => &hash != bh,
            };
            if is_changed { Some(Ok(rel)) } else { None }
        })
        .collect::<io::Result<Vec<_>>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(name);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
            let target = self.0.join(rel);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, data).unwrap();
            target
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn snapshot_empty_dir() {
        let dir = TempDir::new("runtest_snapshot_empty");
        let map = snapshot_directory(dir.path()).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn snapshot_captures_all_files() {
        let dir = TempDir::new("runtest_snapshot_all");
        dir.write("a.txt", b"hello");
        dir.write("sub/b.txt", b"world");
        let map = snapshot_directory(dir.path()).unwrap();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(Path::new("a.txt")));
        assert!(map.contains_key(&PathBuf::from("sub").join("b.txt")));
    }

    #[test]
    fn snapshot_hash_is_content_based() {
        // Same-size files with different content must produce different hashes.
        let dir = TempDir::new("runtest_snapshot_content");
        let p1 = dir.write("file1.txt", b"aaa");
        let p2 = dir.write("file2.txt", b"bbb");
        assert_eq!(
            fs::metadata(&p1).unwrap().len(),
            fs::metadata(&p2).unwrap().len(),
        );
        let h1 = hash_file(&p1).unwrap();
        let h2 = hash_file(&p2).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn changed_files_new_file() {
        let dir = TempDir::new("runtest_changed_new");
        let baseline = snapshot_directory(dir.path()).unwrap();
        dir.write("new.txt", b"hello");
        let changed = changed_files(dir.path(), &baseline).unwrap();
        assert_eq!(changed, vec![PathBuf::from("new.txt")]);
    }

    #[test]
    fn changed_files_modified_content() {
        // Same filename, same size, different bytes — mtime+size would miss this.
        let dir = TempDir::new("runtest_changed_modified");
        dir.write("f.txt", b"aaa");
        let baseline = snapshot_directory(dir.path()).unwrap();
        dir.write("f.txt", b"bbb");
        let changed = changed_files(dir.path(), &baseline).unwrap();
        assert!(changed.contains(&PathBuf::from("f.txt")));
    }

    #[test]
    fn changed_files_unchanged() {
        let dir = TempDir::new("runtest_changed_unchanged");
        dir.write("f.txt", b"hello");
        let baseline = snapshot_directory(dir.path()).unwrap();
        let changed = changed_files(dir.path(), &baseline).unwrap();
        assert!(changed.is_empty());
    }

    #[test]
    fn changed_files_deleted_not_reported() {
        // A file deleted before changed_files is called won't appear in walkdir —
        // deletions are intentionally not tracked.
        let dir = TempDir::new("runtest_changed_deleted");
        dir.write("to_delete.txt", b"x");
        let baseline = snapshot_directory(dir.path()).unwrap();
        fs::remove_file(dir.path().join("to_delete.txt")).unwrap();
        let changed = changed_files(dir.path(), &baseline).unwrap();
        assert!(!changed.contains(&PathBuf::from("to_delete.txt")));
    }

    #[test]
    fn changed_files_empty_baseline() {
        let dir = TempDir::new("runtest_changed_empty_baseline");
        dir.write("a.txt", b"x");
        dir.write("b.txt", b"y");
        let changed = changed_files(dir.path(), &HashMap::new()).unwrap();
        assert_eq!(changed.len(), 2);
    }
}
