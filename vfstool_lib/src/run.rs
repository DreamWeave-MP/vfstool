// SPDX-License-Identifier: GPL-3.0-only
use crate::vfs::VFS;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    hash::BuildHasher,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};
use walkdir::WalkDir;

/// Map from relative file path to its BLAKE3 digest, used to detect changes after a subprocess run.
pub type Snapshot = HashMap<PathBuf, [u8; 32]>;

/// One baseline row for metadata-prefiltered run change detection.
#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    /// BLAKE3 digest captured at baseline time.
    pub hash: [u8; 32],
    /// File size captured at baseline time.
    pub size: u64,
    /// Modification time captured at baseline time, when the platform provides it.
    pub modified: Option<SystemTime>,
}

/// Snapshot with both content hashes and cheap metadata for prefiltering changed files.
pub type MetadataSnapshot = HashMap<PathBuf, SnapshotEntry>;

/// Dump the VFS to `merged_dir` and capture a baseline snapshot.
///
/// Returns the number of files dumped and the snapshot for use with
/// [`run_finalize`]. The caller is responsible for executing the subprocess
/// between these two calls.
///
/// `merged_dir` is created if needed. If it already exists, it is removed
/// recursively before the dump so child processes see only the current VFS
/// contents. Callers should pass a dedicated scratch directory, not a directory
/// containing user data.
///
/// When `use_hardlinks` is `true`, loose files are hardlinked into `merged_dir`.
/// This is intentional for speed and disk usage, but child processes that edit
/// files in place may mutate the original source files through those hardlinks.
///
/// # Errors
///
/// Returns an error if writing merged files or hashing baseline files fails.
pub fn run_setup(
    vfs: &VFS,
    merged_dir: &Path,
    use_hardlinks: bool,
) -> io::Result<(usize, Snapshot)> {
    if merged_dir.exists() {
        std::fs::remove_dir_all(merged_dir)?;
    }
    std::fs::create_dir_all(merged_dir)?;
    let count = vfs.dump_to_directory(merged_dir, use_hardlinks)?;
    let baseline = snapshot_directory(merged_dir)?;
    Ok((count, baseline))
}

/// Dump the VFS to `merged_dir` and capture a metadata-prefiltering baseline snapshot.
///
/// This is the faster variant used by the CLI `run` command. It still records content hashes at
/// baseline time, but later finalization hashes only files whose size or modification time changed.
///
/// # Errors
///
/// Returns an error if writing merged files or capturing baseline metadata/hashes fails.
pub fn run_setup_tracked(
    vfs: &VFS,
    merged_dir: &Path,
    use_hardlinks: bool,
) -> io::Result<(usize, MetadataSnapshot)> {
    if merged_dir.exists() {
        std::fs::remove_dir_all(merged_dir)?;
    }
    std::fs::create_dir_all(merged_dir)?;
    let count = vfs.dump_to_directory(merged_dir, use_hardlinks)?;
    let baseline = snapshot_directory_metadata(merged_dir)?;
    Ok((count, baseline))
}

/// Copy files changed since `baseline` from `merged_dir` into `output_dir`.
///
/// Returns a list of `(relative_path, destination_path)` pairs for every file
/// that was copied. The caller should call this only after the subprocess
/// succeeds.
///
/// # Errors
///
/// Returns an error if hashing changed files or copying outputs fails.
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

/// Copy files changed since `baseline` from `merged_dir` into `output_dir`.
///
/// Uses file metadata as a prefilter and hashes only new files or files whose metadata differs
/// from the baseline. Deletions are still ignored.
///
/// # Errors
///
/// Returns an error if reading metadata, hashing changed candidates, or copying outputs fails.
pub fn run_finalize_tracked(
    merged_dir: &Path,
    baseline: &MetadataSnapshot,
    output_dir: &Path,
) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let changed = changed_files_metadata(merged_dir, baseline)?;
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
/// Uses a 64 KiB heap buffer to avoid loading the whole file into memory.
///
/// # Errors
///
/// Returns an error if opening or reading the file fails.
pub fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn snapshot_entry(path: &Path) -> io::Result<SnapshotEntry> {
    let metadata = std::fs::metadata(path)?;
    Ok(SnapshotEntry {
        hash: hash_file(path)?,
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// Hash every file under `dir` in parallel.
/// Returns a map from relative path to its BLAKE3 digest.
///
/// # Errors
///
/// Returns an error if directory traversal or hashing fails.
pub fn snapshot_directory(dir: &Path) -> io::Result<Snapshot> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|entry| match entry.map_err(io::Error::other) {
            Ok(entry) if entry.file_type().is_file() => Some(Ok(entry)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .par_bridge()
        .map(|entry| {
            let entry = entry?;
            let rel = entry
                .path()
                .strip_prefix(dir)
                .map_err(|_| io::Error::other("walkdir entry should be under root"))?
                .to_path_buf();
            let hash = hash_file(entry.path())?;
            Ok((rel, hash))
        })
        .collect::<io::Result<Snapshot>>()
}

/// Hash every file under `dir` in parallel and record cheap metadata alongside the digest.
///
/// # Errors
///
/// Returns an error if directory traversal, metadata reads, or hashing fails.
pub fn snapshot_directory_metadata(dir: &Path) -> io::Result<MetadataSnapshot> {
    WalkDir::new(dir)
        .into_iter()
        .filter_map(|entry| match entry.map_err(io::Error::other) {
            Ok(entry) if entry.file_type().is_file() => Some(Ok(entry)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .par_bridge()
        .map(|entry| {
            let entry = entry?;
            let rel = entry
                .path()
                .strip_prefix(dir)
                .map_err(|_| io::Error::other("walkdir entry should be under root"))?
                .to_path_buf();
            let snapshot = snapshot_entry(entry.path())?;
            Ok((rel, snapshot))
        })
        .collect::<io::Result<MetadataSnapshot>>()
}

/// Walk `dir` and return relative paths of files whose content differs from `baseline`.
/// Files in `baseline` that no longer exist in `dir` are silently ignored.
///
/// # Errors
///
/// Returns an error if directory traversal or hashing fails.
pub fn changed_files<S: BuildHasher + Sync>(
    dir: &Path,
    baseline: &HashMap<PathBuf, [u8; 32], S>,
) -> io::Result<Vec<PathBuf>> {
    let mut changed = WalkDir::new(dir)
        .into_iter()
        .filter_map(|entry| match entry.map_err(io::Error::other) {
            Ok(entry) if entry.file_type().is_file() => Some(Ok(entry)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .par_bridge()
        .filter_map(|entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => return Some(Err(err)),
            };
            let rel = match entry.path().strip_prefix(dir) {
                Ok(path) => path.to_path_buf(),
                Err(_) => return Some(Err(io::Error::other("walkdir entry should be under root"))),
            };
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
        .collect::<io::Result<Vec<_>>>()?;
    changed.sort();
    Ok(changed)
}

/// Walk `dir` and return relative paths whose metadata or content differs from `baseline`.
///
/// Files whose size and modification time match the baseline are treated as unchanged without
/// rehashing. Files with changed metadata are hashed to avoid reporting metadata-only touches.
///
/// # Errors
///
/// Returns an error if directory traversal, metadata reads, or hashing changed candidates fails.
pub fn changed_files_metadata<S: BuildHasher + Sync>(
    dir: &Path,
    baseline: &HashMap<PathBuf, SnapshotEntry, S>,
) -> io::Result<Vec<PathBuf>> {
    let mut changed = WalkDir::new(dir)
        .into_iter()
        .filter_map(|entry| match entry.map_err(io::Error::other) {
            Ok(entry) if entry.file_type().is_file() => Some(Ok(entry)),
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .par_bridge()
        .filter_map(|entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => return Some(Err(err)),
            };
            let rel = match entry.path().strip_prefix(dir) {
                Ok(path) => path.to_path_buf(),
                Err(_) => return Some(Err(io::Error::other("walkdir entry should be under root"))),
            };
            let Some(baseline_entry) = baseline.get(&rel) else {
                return Some(Ok(rel));
            };
            let metadata = match std::fs::metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(err) => return Some(Err(err)),
            };
            let modified = metadata.modified().ok();
            if metadata.len() == baseline_entry.size && modified == baseline_entry.modified {
                return None;
            }
            let hash = match hash_file(entry.path()) {
                Ok(hash) => hash,
                Err(err) => return Some(Err(err)),
            };
            (hash != baseline_entry.hash).then_some(Ok(rel))
        })
        .collect::<io::Result<Vec<_>>>()?;
    changed.sort();
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "{name}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
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

    #[test]
    fn changed_files_are_sorted() {
        let dir = TempDir::new("runtest_changed_sorted");
        dir.write("z.txt", b"z");
        dir.write("a.txt", b"a");
        dir.write("m.txt", b"m");

        let changed = changed_files(dir.path(), &HashMap::new()).unwrap();
        assert_eq!(
            changed,
            vec![
                PathBuf::from("a.txt"),
                PathBuf::from("m.txt"),
                PathBuf::from("z.txt")
            ]
        );
    }

    #[test]
    fn metadata_snapshot_captures_hash_and_size() {
        let dir = TempDir::new("runtest_metadata_snapshot");
        dir.write("a.txt", b"hello");

        let snapshot = snapshot_directory_metadata(dir.path()).unwrap();

        let entry = snapshot.get(Path::new("a.txt")).unwrap();
        assert_eq!(entry.size, 5);
        assert_eq!(entry.hash, hash_file(&dir.path().join("a.txt")).unwrap());
    }

    #[test]
    fn changed_files_metadata_reports_new_files() {
        let dir = TempDir::new("runtest_metadata_changed_new");
        let baseline = snapshot_directory_metadata(dir.path()).unwrap();
        dir.write("new.txt", b"hello");

        let changed = changed_files_metadata(dir.path(), &baseline).unwrap();

        assert_eq!(changed, vec![PathBuf::from("new.txt")]);
    }

    #[test]
    fn changed_files_metadata_ignores_unchanged_files() {
        let dir = TempDir::new("runtest_metadata_changed_unchanged");
        dir.write("same.txt", b"hello");
        let baseline = snapshot_directory_metadata(dir.path()).unwrap();

        let changed = changed_files_metadata(dir.path(), &baseline).unwrap();

        assert!(changed.is_empty());
    }

    #[test]
    fn changed_files_metadata_reports_modified_size() {
        let dir = TempDir::new("runtest_metadata_changed_size");
        dir.write("f.txt", b"hello");
        let baseline = snapshot_directory_metadata(dir.path()).unwrap();
        dir.write("f.txt", b"hello world");

        let changed = changed_files_metadata(dir.path(), &baseline).unwrap();

        assert_eq!(changed, vec![PathBuf::from("f.txt")]);
    }

    // ---- run_setup ----

    #[test]
    fn run_setup_creates_merged_dir() {
        let src = TempDir::new("run_new_setup_src");
        src.write("file.txt", b"hello");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let base = TempDir::new("run_new_setup_base");
        let merged = base.path().join("merged_does_not_exist");
        assert!(!merged.exists());

        run_setup(&vfs, &merged, false).unwrap();
        assert!(merged.exists(), "run_setup should create merged_dir");
    }

    #[test]
    fn run_setup_count_matches_vfs_size() {
        let src = TempDir::new("run_new_setup_count_src");
        src.write("a.txt", b"1");
        src.write("b.txt", b"2");
        src.write("sub/c.txt", b"3");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_setup_count_merged");
        let (count, _) = run_setup(&vfs, merged.path(), false).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn run_setup_returns_non_empty_snapshot() {
        let src = TempDir::new("run_new_setup_snap_src");
        src.write("file.txt", b"data");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_setup_snap_merged");
        let (_, snapshot) = run_setup(&vfs, merged.path(), false).unwrap();
        assert!(
            !snapshot.is_empty(),
            "snapshot should contain entries after setup"
        );
    }

    #[test]
    fn run_setup_tracked_returns_metadata_snapshot() {
        let src = TempDir::new("run_new_setup_tracked_src");
        src.write("file.txt", b"data");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_setup_tracked_merged");
        let (_, snapshot) = run_setup_tracked(&vfs, merged.path(), false).unwrap();

        assert!(snapshot.contains_key(Path::new("file.txt")));
    }

    #[test]
    fn run_setup_clears_preexisting_merged_files() {
        let src = TempDir::new("run_new_setup_existing_src");
        src.write("file.txt", b"data");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_setup_existing_merged");
        merged.write("preexisting.txt", b"keep");

        let (_, snapshot) = run_setup(&vfs, merged.path(), false).unwrap();
        assert!(snapshot.contains_key(Path::new("file.txt")));
        assert!(!snapshot.contains_key(Path::new("preexisting.txt")));
        assert!(!merged.path().join("preexisting.txt").exists());
    }

    // ---- run_finalize ----

    #[test]
    fn run_finalize_empty_when_nothing_changed() {
        let src = TempDir::new("run_new_finalize_nochange_src");
        src.write("file.txt", b"data");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_finalize_nochange_merged");
        let (_, baseline) = run_setup(&vfs, merged.path(), false).unwrap();

        let output = TempDir::new("run_new_finalize_nochange_out");
        let copied = run_finalize(merged.path(), &baseline, output.path()).unwrap();
        assert!(
            copied.is_empty(),
            "nothing changed so nothing should be copied"
        );
    }

    #[test]
    fn run_finalize_copies_modified_file() {
        let src = TempDir::new("run_new_finalize_mod_src");
        src.write("file.txt", b"original");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_finalize_mod_merged");
        let (_, baseline) = run_setup(&vfs, merged.path(), false).unwrap();

        // Modify the file in merged_dir after baseline
        fs::write(merged.path().join("file.txt"), b"modified").unwrap();

        let output = TempDir::new("run_new_finalize_mod_out");
        let copied = run_finalize(merged.path(), &baseline, output.path()).unwrap();

        assert!(!copied.is_empty(), "modified file should be copied");
        let (rel, dest) = &copied[0];
        assert_eq!(rel, &PathBuf::from("file.txt"));
        assert_eq!(fs::read(dest).unwrap(), b"modified");
    }

    #[test]
    fn run_finalize_tracked_copies_modified_file() {
        let src = TempDir::new("run_new_finalize_tracked_mod_src");
        src.write("file.txt", b"original");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let merged = TempDir::new("run_new_finalize_tracked_mod_merged");
        let (_, baseline) = run_setup_tracked(&vfs, merged.path(), false).unwrap();

        fs::write(merged.path().join("file.txt"), b"modified with new size").unwrap();

        let output = TempDir::new("run_new_finalize_tracked_mod_out");
        let copied = run_finalize_tracked(merged.path(), &baseline, output.path()).unwrap();

        assert_eq!(copied.len(), 1);
        let (rel, dest) = &copied[0];
        assert_eq!(rel, &PathBuf::from("file.txt"));
        assert_eq!(fs::read(dest).unwrap(), b"modified with new size");
    }
}
