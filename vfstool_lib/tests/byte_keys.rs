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

#[cfg(unix)]
#[test]
fn analysis_reports_carry_keys_that_are_not_utf8_exactly() {
    use std::os::unix::ffi::OsStrExt;
    let low = TempDir::new("analysis_low");
    let high = TempDir::new("analysis_high");
    let name = std::ffi::OsStr::from_bytes(b"odd/\xffkey.dat");
    for (dir, content) in [(&low, &b"low"[..]), (&high, &b"high"[..])] {
        let path = dir.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
    }
    let (mut vfs, layer) =
        VFS::from_directories_with_layer_index([low.0.as_path(), high.0.as_path()], None);
    let key_path = PathBuf::from(std::ffi::OsStr::from_bytes(b"odd/\xffkey.dat"));

    let explain = vfs
        .explain(&b"odd/\xffkey.dat"[..])
        .expect("the key resolves");
    assert_eq!(explain.key, key_path, "report keys keep their bytes");
    assert_eq!(explain.winner.key, key_path);
    assert_eq!(vfs.duplicates().entries[0].key, key_path);
    let chain = layer.provider_chain(&key_path);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[1].key, key_path);
    // The lock manifest and semantic conflicts look every key up again by its path; a lossy
    // spelling used to drop such keys from both.
    let lock = layer.lock_manifest(&vfs).unwrap();
    assert_eq!(lock.entries.len(), 1);
    assert_eq!(lock.entries[0].key, key_path);
    assert_eq!(lock.entries[0].winner_size, Some(4));
    let semantic = layer.semantic_conflicts(&vfs).unwrap();
    assert_eq!(semantic.entries.len(), 1);
    assert_eq!(semantic.entries[0].distinct_versions, 2);
    let provenance = layer
        .provenance(&vfs, &key_path, true)
        .unwrap()
        .expect("provenance");
    assert_eq!(provenance.providers[1].size, Some(4));
    assert_eq!(vfs.remove_resolved_matching_glob("odd/*").len(), 1);
}

#[cfg(all(unix, feature = "serialize"))]
#[test]
fn reports_holding_keys_that_are_not_utf8_serialize_and_a_lock_reads_back_without_drift() {
    use std::os::unix::ffi::OsStrExt;
    use vfstool_lib::{CollapseOptions, SerializeType, VfsLock, serde_json, serialize_value};

    let low = TempDir::new("serialize_low");
    let high = TempDir::new("serialize_high");
    let name = std::ffi::OsStr::from_bytes(b"Textures/caf\xe9.dds");
    for (dir, content) in [(&low, &b"low"[..]), (&high, &b"high"[..])] {
        let path = dir.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
    }
    let dirs = [low.0.as_path(), high.0.as_path()];
    let (vfs, layer) = VFS::from_directories_with_layer_index(dirs, None);
    let (_, conflicts) = VFS::from_directories_with_conflict_index(dirs, None);
    let key = &b"textures/caf\xe9.dds"[..];
    let text_key = "textures/caf\u{fffd}.dds";

    for format in [
        SerializeType::Json,
        SerializeType::Yaml,
        SerializeType::Toml,
    ] {
        let explain = serialize_value(&vfs.explain(key).unwrap(), format).unwrap();
        assert!(explain.contains(text_key), "{explain}");
        serialize_value(&vfs.duplicates(), format).unwrap();
        serialize_value(&conflicts.conflicts_report(false), format).unwrap();
        serialize_value(&conflicts.shadowed_report_with_files(false, true), format).unwrap();
        let plan = vfs.materialization_plan(
            low.0.join("merged"),
            &CollapseOptions {
                allow_copying: false,
                extract_archives: false,
                use_symlinks: false,
            },
        );
        serialize_value(&plan, format).unwrap();
    }

    let lock = layer.lock_manifest(&vfs).unwrap();
    let written = serialize_value(&lock, SerializeType::Json).unwrap();
    assert!(written.contains(text_key), "{written}");
    let read: VfsLock = serde_json::from_str(&written).unwrap();
    let drift = layer.diff_against_lock(&vfs, &read).unwrap();
    assert!(drift.entries.is_empty(), "{:?}", drift.entries);
    serialize_value(&drift, SerializeType::Json).unwrap();

    fs::write(high.0.join(name), b"changed").unwrap();
    let drift = layer.diff_against_lock(&vfs, &read).unwrap();
    assert_eq!(drift.entries.len(), 1, "{:?}", drift.entries);
    assert_eq!(
        drift.entries[0].key,
        PathBuf::from(std::ffi::OsStr::from_bytes(b"textures/caf\xe9.dds"))
    );
    assert!(
        serialize_value(&drift, SerializeType::Json)
            .unwrap()
            .contains(text_key)
    );
}
