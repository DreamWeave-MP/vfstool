use super::*;

// ---- extract_file ----

#[test]
fn extract_file_returns_none_for_missing_path() {
    let dir = TempDir::new("vfs_newmethods_extract_none");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let dest = TempDir::new("vfs_newmethods_extract_none_dest");
    let result = vfs
        .extract_file(Path::new("nonexistent.txt"), dest.path())
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn extract_file_copies_content_to_dest() {
    let src = TempDir::new("vfs_newmethods_extract_src");
    src.write("data.bin", b"hello extract");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("vfs_newmethods_extract_dest");
    let extracted = vfs
        .extract_file(Path::new("data.bin"), dest.path())
        .unwrap()
        .expect("file should be found and extracted");

    assert!(extracted.exists());
    assert_eq!(fs::read(&extracted).unwrap(), b"hello extract");
}

#[test]
#[cfg(unix)]
fn extract_file_does_not_follow_existing_destination_symlink() {
    let src = TempDir::new("vfs_newmethods_extract_symlink_src");
    src.write("data.bin", b"safe");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("vfs_newmethods_extract_symlink_dest");
    let outside = dest.write("outside.bin", b"outside");
    std::os::unix::fs::symlink(&outside, dest.path().join("data.bin")).unwrap();

    let extracted = vfs
        .extract_file(Path::new("data.bin"), dest.path())
        .unwrap()
        .expect("file should be extracted");

    assert_eq!(fs::read(&outside).unwrap(), b"outside");
    assert_eq!(fs::read(&extracted).unwrap(), b"safe");
    assert!(
        !fs::symlink_metadata(&extracted)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn extract_file_creates_dest_dir_if_missing() {
    let src = TempDir::new("vfs_newmethods_extract_mkdest");
    src.write("note.txt", b"content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    // Use a dest dir that does not yet exist
    let base = TempDir::new("vfs_newmethods_extract_mkdest_base");
    let dest = base.path().join("new_subdir");
    assert!(!dest.exists(), "dest should not exist yet");

    let extracted = vfs
        .extract_file(Path::new("note.txt"), &dest)
        .unwrap()
        .expect("file should be extracted");

    assert!(extracted.exists());
}

// ---- remaining ----

#[test]
fn remaining_replacements_false_includes_files_from_filter_path() {
    let dir1 = TempDir::new("vfs_newmethods_rem_d1");
    let dir2 = TempDir::new("vfs_newmethods_rem_d2");

    // dir1 has unique.txt (not overridden by dir2)
    dir1.write("unique.txt", b"from dir1");
    // both have shared.txt — dir2 wins
    dir1.write("shared.txt", b"dir1 version");
    dir2.write("shared.txt", b"dir2 version");
    dir2.write("only_dir2.txt", b"dir2 only");

    let all_dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
    let vfs = VFS::from_directories(all_dirs.iter().map(std::path::PathBuf::as_path), None);

    // replacements_only=false: files still served FROM dir1
    let tree = vfs.remaining(dir1.path(), false, &all_dirs, true);
    let count = count_files_in_tree(&tree);
    // unique.txt is served from dir1; shared.txt is NOT (dir2 wins)
    assert_eq!(count, 1, "only unique.txt should still be served from dir1");
}

#[test]
fn remaining_replacements_true_includes_overridden_files() {
    let dir1 = TempDir::new("vfs_newmethods_rem2_d1");
    let dir2 = TempDir::new("vfs_newmethods_rem2_d2");

    // dir1 has unique.txt and shared.txt; dir2 overrides shared.txt
    dir1.write("unique.txt", b"from dir1");
    dir1.write("shared.txt", b"dir1 version");
    dir2.write("shared.txt", b"dir2 version");

    let all_dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
    let vfs = VFS::from_directories(all_dirs.iter().map(std::path::PathBuf::as_path), None);

    // replacements_only=true: files that dir1 has but a higher-priority dir wins
    let tree = vfs.remaining(dir1.path(), true, &all_dirs, true);
    let count = count_files_in_tree(&tree);
    // shared.txt is in dir1 but served from dir2 — it should appear
    // unique.txt is in dir1 and still served from dir1 — it should NOT appear
    assert_eq!(count, 1, "only the overridden shared.txt should appear");
}
