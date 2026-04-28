use super::*;
use crate::CollapseOptions;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

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
fn dump_creates_files() {
    let src = TempDir::new("dump_creates_src");
    src.write("textures/foo.dds", b"dds_data");
    src.write("meshes/bar.nif", b"nif_data");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_creates_dest");
    vfs.dump_to_directory(dest.path(), false).unwrap();

    assert_eq!(
        fs::read(dest.path().join("textures/foo.dds")).unwrap(),
        b"dds_data"
    );
    assert_eq!(
        fs::read(dest.path().join("meshes/bar.nif")).unwrap(),
        b"nif_data"
    );
}

#[test]
fn dump_creates_subdirectories() {
    let src = TempDir::new("dump_subdirs_src");
    src.write("a/b/c/deep.txt", b"deep");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_subdirs_dest");
    vfs.dump_to_directory(dest.path(), false).unwrap();

    assert!(dest.path().join("a/b/c/deep.txt").exists());
}

#[test]
#[cfg(unix)]
fn materialization_preserves_non_utf8_key_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let file_name = OsString::from_vec(vec![b'f', 0xff, b'o', b'.', b'd', b'd', b's']);
    let src = TempDir::new("dump_non_utf8_src");
    fs::write(src.path().join(&file_name), b"bytes").unwrap();
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dump_dest = TempDir::new("dump_non_utf8_dest");
    assert_eq!(vfs.dump_to_directory(dump_dest.path(), false).unwrap(), 1);
    assert_eq!(
        fs::read(dump_dest.path().join(&file_name)).unwrap(),
        b"bytes"
    );

    let collapse_dest = TempDir::new("collapse_non_utf8_dest");
    vfs.collapse_into(
        collapse_dest.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(collapse_dest.path().join(&file_name)).unwrap(),
        b"bytes"
    );

    let extract_dest = TempDir::new("extract_non_utf8_dest");
    let extracted = vfs
        .extract_file(&PathBuf::from(file_name.clone()), extract_dest.path())
        .unwrap()
        .unwrap();
    assert_eq!(extracted, extract_dest.path().join(&file_name));
    assert_eq!(fs::read(extracted).unwrap(), b"bytes");
}

#[test]
fn dump_count_accurate() {
    let src = TempDir::new("dump_count_src");
    src.write("a.txt", b"");
    src.write("b.txt", b"");
    src.write("sub/c.txt", b"");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_count_dest");
    let count = vfs.dump_to_directory(dest.path(), false).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn dump_rejects_file_directory_key_conflicts() {
    let src = TempDir::new("dump_path_conflict_src");
    let file = src.write("file_source", b"file");
    let nested = src.write("nested_source", b"nested");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("foo", &file);
    vfs.set_winner_loose_file("foo/bar.txt", &nested);

    let dest = TempDir::new("dump_path_conflict_dest");
    let err = vfs
        .dump_to_directory(dest.path(), false)
        .expect_err("path conflict should be rejected");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("cannot both be materialized"));
}

#[test]
fn collapse_rejects_file_directory_key_conflicts() {
    let src = TempDir::new("collapse_path_conflict_src");
    let file = src.write("file_source", b"file");
    let nested = src.write("nested_source", b"nested");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("foo", &file);
    vfs.set_winner_loose_file("foo/bar.txt", &nested);

    let dest = TempDir::new("collapse_path_conflict_dest");
    let err = vfs
        .collapse_into(
            dest.path(),
            &CollapseOptions {
                allow_copying: true,
                extract_archives: true,
                use_symlinks: false,
            },
        )
        .expect_err("path conflict should be rejected");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("cannot both be materialized"));
}

#[test]
fn collapse_reports_missing_loose_source() {
    let src = TempDir::new("collapse_missing_loose_src");
    let missing = src.path().join("missing.txt");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("missing.txt", &missing);

    let dest = TempDir::new("collapse_missing_loose_dest");
    let err = vfs
        .collapse_into(
            dest.path(),
            &CollapseOptions {
                allow_copying: true,
                extract_archives: true,
                use_symlinks: false,
            },
        )
        .expect_err("missing source should fail collapse");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(err.to_string().contains("source file no longer exists"));
}

#[test]
#[cfg(feature = "zip")]
fn collapse_extract_archives_skips_loose_archive_without_deleting_existing_output() {
    let src = TempDir::new("collapse_skip_loose_archive_src");
    src.write("data.zip", b"source archive");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("collapse_skip_loose_archive_dest");
    dest.write("data.zip", b"existing output");

    vfs.collapse_into(
        dest.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    )
    .unwrap();

    assert_eq!(
        fs::read(dest.path().join("data.zip")).unwrap(),
        b"existing output"
    );
}

#[test]
#[cfg(not(feature = "zip"))]
fn collapse_extract_archives_copies_loose_zip_when_zip_feature_is_disabled() {
    let src = TempDir::new("collapse_copy_loose_zip_without_feature_src");
    src.write("data.zip", b"source archive");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("collapse_copy_loose_zip_without_feature_dest");

    vfs.collapse_into(
        dest.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    )
    .unwrap();

    assert_eq!(
        fs::read(dest.path().join("data.zip")).unwrap(),
        b"source archive"
    );
}

#[test]
fn dump_copy_mode() {
    let src = TempDir::new("dump_copy_src");
    src.write("data.txt", b"hello world");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_copy_dest");
    vfs.dump_to_directory(dest.path(), false).unwrap();
    assert_eq!(
        fs::read(dest.path().join("data.txt")).unwrap(),
        b"hello world"
    );
}

#[test]
#[cfg(unix)]
fn dump_hardlink_mode() {
    use std::os::unix::fs::MetadataExt;
    let src = TempDir::new("dump_hardlink_src");
    src.write("data.txt", b"linked");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_hardlink_dest");
    vfs.dump_to_directory(dest.path(), true).unwrap();

    let meta = fs::metadata(dest.path().join("data.txt")).unwrap();
    assert!(meta.nlink() >= 2, "hardlinked file should have nlink >= 2");
}

#[test]
fn dump_skips_missing_source() {
    let src = TempDir::new("dump_missing_src");
    let gone = src.write("gone.txt", b"x");
    let vfs = VFS::from_directories(vec![src.path()], None);

    // Delete source file before dumping
    fs::remove_file(&gone).unwrap();

    let dest = TempDir::new("dump_missing_dest");
    let count = vfs.dump_to_directory(dest.path(), false).unwrap();
    assert_eq!(count, 0);
    assert!(!dest.path().join("gone.txt").exists());
}

#[test]
fn dump_overwrites_existing() {
    let src = TempDir::new("dump_overwrite_src");
    src.write("f.txt", b"new_content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_overwrite_dest");
    dest.write("f.txt", b"old_content");

    vfs.dump_to_directory(dest.path(), false).unwrap();
    assert_eq!(fs::read(dest.path().join("f.txt")).unwrap(), b"new_content");
}

#[test]
#[cfg(unix)]
fn dump_copy_does_not_follow_existing_destination_symlink() {
    let src = TempDir::new("dump_symlink_src");
    src.write("f.txt", b"new_content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_symlink_dest");
    let outside = dest.write("outside.txt", b"outside");
    std::os::unix::fs::symlink(&outside, dest.path().join("f.txt")).unwrap();

    vfs.dump_to_directory(dest.path(), false).unwrap();
    assert_eq!(fs::read(&outside).unwrap(), b"outside");
    assert_eq!(fs::read(dest.path().join("f.txt")).unwrap(), b"new_content");
    assert!(
        !fs::symlink_metadata(dest.path().join("f.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
#[cfg(unix)]
fn dump_rejects_symlinked_parent_directory() {
    let src = TempDir::new("dump_parent_symlink_src");
    src.write("textures/f.txt", b"new_content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_parent_symlink_dest");
    let outside = TempDir::new("dump_parent_symlink_outside");
    std::os::unix::fs::symlink(outside.path(), dest.path().join("textures")).unwrap();

    let err = vfs
        .dump_to_directory(dest.path(), false)
        .expect_err("symlinked parent should be rejected");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(!outside.path().join("f.txt").exists());
}

#[test]
#[cfg(unix)]
fn collapse_rejects_symlinked_parent_directory() {
    let src = TempDir::new("collapse_parent_symlink_src");
    src.write("textures/f.txt", b"new_content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("collapse_parent_symlink_dest");
    let outside = TempDir::new("collapse_parent_symlink_outside");
    std::os::unix::fs::symlink(outside.path(), dest.path().join("textures")).unwrap();

    let err = vfs
        .collapse_into(
            dest.path(),
            &CollapseOptions {
                allow_copying: true,
                extract_archives: true,
                use_symlinks: false,
            },
        )
        .expect_err("symlinked parent should be rejected");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(!outside.path().join("f.txt").exists());
}

#[test]
#[cfg(unix)]
fn materialization_plan_reports_symlinked_parent_directory() {
    let src = TempDir::new("plan_parent_symlink_src");
    src.write("textures/f.txt", b"new_content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("plan_parent_symlink_dest");
    let outside = TempDir::new("plan_parent_symlink_outside");
    std::os::unix::fs::symlink(outside.path(), dest.path().join("textures")).unwrap();

    let plan = vfs.materialization_plan(
        dest.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    );

    assert!(plan.actions.is_empty());
    assert_eq!(plan.issues.len(), 1);
}

#[test]
#[cfg(unix)]
fn dump_hardlink_overwrites_existing() {
    use std::os::unix::fs::MetadataExt;
    let src = TempDir::new("dump_hardlink_overwrite_src");
    src.write("f.txt", b"new_content");
    let vfs = VFS::from_directories(vec![src.path()], None);

    let dest = TempDir::new("dump_hardlink_overwrite_dest");
    dest.write("f.txt", b"old_content");

    vfs.dump_to_directory(dest.path(), true).unwrap();
    assert_eq!(fs::read(dest.path().join("f.txt")).unwrap(), b"new_content");
    assert!(fs::metadata(dest.path().join("f.txt")).unwrap().nlink() >= 2);
}

#[test]
#[cfg(feature = "zip")]
fn dump_archive_files_correct_content() {
    use std::io::Write as IoWrite;
    let src = TempDir::new("dump_zip_src");

    // Create a ZIP archive with one entry.
    let zip_path = src.path().join("data.zip");
    {
        let file = fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("scripts/test.lua", options).unwrap();
        writer.write_all(b"return 42").unwrap();
        writer.finish().unwrap();
    }

    let vfs = VFS::from_directories(vec![src.path()], Some(vec!["data.zip"]));
    let dest = TempDir::new("dump_zip_dest");
    vfs.dump_to_directory(dest.path(), false).unwrap();

    let content = fs::read(dest.path().join("scripts/test.lua")).unwrap();
    assert_eq!(content, b"return 42");
}
