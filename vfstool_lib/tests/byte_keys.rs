// SPDX-License-Identifier: MIT OR Apache-2.0
//! Byte-first key lookups: keys need no UTF-8, already-normalized input borrows, and the
//! normalized-key entry points find what the scratch-normalizing caller asks for.

use std::{borrow::Cow, fs, path::PathBuf};

use vfstool_lib::{VFS, VfsFile, VfsKeyInput};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("vfstool_byte_keys_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn key_inputs_borrow_when_already_normalized() {
    assert!(matches!(
        "textures/foo.dds".vfs_key_bytes(),
        Cow::Borrowed(_)
    ));
    assert!(matches!(
        b"textures/foo.dds"[..].vfs_key_bytes(),
        Cow::Borrowed(_)
    ));
    assert!(matches!(
        PathBuf::from("textures/foo.dds").vfs_key_bytes(),
        Cow::Borrowed(_)
    ));
    let mixed = "Textures\\Foo.DDS".vfs_key_bytes();
    assert!(matches!(mixed, Cow::Owned(_)));
    assert_eq!(mixed.as_ref(), b"textures/foo.dds");
    assert_eq!(
        b"//Textures\\\\Foo.DDS"[..].vfs_key_bytes().as_ref(),
        b"textures/foo.dds"
    );
}

#[test]
fn lookups_accept_bytes_normalized_keys_and_report_length() {
    let dir = TempDir::new("lookups");
    let foo = dir.write("Textures/Foo.DDS", b"foo");
    dir.write("meshes/bar.nif", b"bar");
    let vfs = VFS::from_directories([dir.0.as_path()], None);

    assert_eq!(vfs.len(), 2);
    assert!(!vfs.is_empty());
    assert!(VFS::new().is_empty());
    assert_eq!(VFS::new().len(), 0);

    assert_eq!(vfs.get_file(&b"TEXTURES\\FOO.DDS"[..]).unwrap().path(), foo);
    assert!(vfs.contains(&b"meshes/bar.nif"[..]));
    assert!(vfs.get_file_normalized(b"textures/foo.dds").is_some());
    assert!(vfs.contains_normalized(b"textures/foo.dds"));
    assert!(vfs.get_file_normalized(b"textures/missing.dds").is_none());
    assert!(!vfs.contains_normalized(b"nope"));

    // The byte-first substring search folds the needle like a host path and compares bytes.
    assert_eq!(vfs.paths_matching_bytes(b"FOO").count(), 1);
    assert_eq!(vfs.paths_matching_bytes(b"Textures\\").count(), 1);
    assert_eq!(vfs.paths_matching("bar").count(), 1);
    assert_eq!(vfs.paths_matching("nothing").count(), 0);
    assert_eq!(vfs.paths_with(&b"Textures\\"[..]).count(), 1);
}

#[cfg(unix)]
#[test]
fn keys_and_paths_that_are_not_utf8_survive_byte_for_byte() {
    use std::os::unix::ffi::OsStrExt;
    let dir = TempDir::new("bytes");
    let name = std::ffi::OsStr::from_bytes(b"Textures/\xffOdd.DDS");
    let path = dir.0.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"odd").unwrap();
    let vfs = VFS::from_directories([dir.0.as_path()], None);

    let file = vfs
        .get_file(&b"textures/\xffodd.dds"[..])
        .expect("a byte key is a key");
    assert_eq!(file.path_bytes(), path.as_os_str().as_bytes());
    assert!(vfs.contains_normalized(b"textures/\xffodd.dds"));
    assert_eq!(vfs.paths_matching_bytes(b"\xffODD").count(), 1);
    assert_eq!(vfs.paths_matching("odd").count(), 1);
    assert_eq!(VfsFile::from("/a/b.txt").path_bytes(), b"/a/b.txt");
}
