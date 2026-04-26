use super::*;
use crate::DirectoryNode;
use std::{
    fs,
    io::Write as IoWrite,
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

    /// Create a ZIP file at `filename` (relative to this dir) with the given entries.
    fn create_zip(&self, filename: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = self.0.join(filename);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tree_contains_file(node: &DirectoryNode, name: &str) -> bool {
    node.files
        .iter()
        .any(|f| f.file_name().is_some_and(|n| n == name))
        || node
            .subdirs
            .values()
            .any(|sub| tree_contains_file(sub, name))
}

fn find_tree_file<'a>(node: &'a DirectoryNode, name: &str) -> Option<&'a VfsFile> {
    node.files
        .iter()
        .find(|f| f.file_name().is_some_and(|n| n == name))
        .or_else(|| {
            node.subdirs
                .values()
                .find_map(|sub| find_tree_file(sub, name))
        })
}

// ---- Construction ----

#[test]
fn zip_entries_appear_in_vfs() {
    let dir = TempDir::new("vfszip_entries");
    dir.create_zip(
        "data.zip",
        &[("textures/foo.dds", b""), ("meshes/bar.nif", b"")],
    );

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

    assert!(vfs.get_file("textures/foo.dds").is_some());
    assert!(vfs.get_file("meshes/bar.nif").is_some());
}

#[test]
fn zip_entries_all_reachable() {
    // Verify all ZIP entries are in the VFS (the zip file itself also appears
    // as a loose entry since the data dir is walked, so we don't count total entries).
    let dir = TempDir::new("vfszip_count");
    dir.create_zip(
        "data.zip",
        &[("a.txt", b""), ("b.txt", b""), ("sub/c.txt", b"")],
    );

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    assert!(vfs.get_file("a.txt").is_some());
    assert!(vfs.get_file("b.txt").is_some());
    assert!(vfs.get_file("sub/c.txt").is_some());
}

// ---- open() / content ----

#[test]
fn zip_entry_content_readable() {
    let dir = TempDir::new("vfszip_content");
    dir.create_zip("data.zip", &[("scripts/hello.lua", b"return 42")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let file = vfs.get_file("scripts/hello.lua").unwrap();

    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
    assert_eq!(buf, b"return 42");
}

#[test]
fn zip_entries_with_unsafe_paths_are_skipped() {
    let dir = TempDir::new("vfszip_unsafe_paths");
    dir.create_zip(
        "data.zip",
        &[
            ("../outside.txt", b"bad"),
            ("/absolute.txt", b"bad"),
            ("safe/inside.txt", b"good"),
        ],
    );
    let out = TempDir::new("vfszip_unsafe_paths_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let dumped = vfs.dump_to_directory(out.path(), false).unwrap();

    assert!(vfs.get_file("../outside.txt").is_none());
    assert!(vfs.get_file("/absolute.txt").is_none());
    assert!(vfs.get_file("safe/inside.txt").is_some());
    assert!(dumped > 0);
    assert!(!out.path().join("../outside.txt").exists());
}

#[test]
fn zip_entry_open_is_repeatable() {
    // open() must be callable multiple times (mutex lock, not move)
    let dir = TempDir::new("vfszip_repeat");
    dir.create_zip("data.zip", &[("foo.dat", b"hello")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let file = vfs.get_file("foo.dat").unwrap();

    for _ in 0..3 {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
        assert_eq!(buf, b"hello");
    }
}

// ---- Priority ----

#[test]
fn loose_file_overrides_zip_entry() {
    let dir = TempDir::new("vfszip_priority");
    dir.create_zip("data.zip", &[("textures/foo.dds", b"from_zip")]);
    let loose = dir.write("textures/foo.dds", b"from_loose");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

    let file = vfs.get_file("textures/foo.dds").unwrap();
    assert!(file.is_loose(), "loose file must win over ZIP entry");
    assert_eq!(file.path(), loose);
}

#[test]
fn later_dir_wins_over_zip_entry() {
    let archive_dir = TempDir::new("vfszip_prio_archive");
    archive_dir.create_zip("data.zip", &[("shared.txt", b"from_zip")]);

    let mod_dir = TempDir::new("vfszip_prio_mod");
    let loose = mod_dir.write("shared.txt", b"from_mod");

    let vfs = VFS::from_directories(
        vec![archive_dir.path(), mod_dir.path()],
        Some(vec!["data.zip"]),
    );

    let file = vfs.get_file("shared.txt").unwrap();
    assert_eq!(file.path(), loose, "loose dir entry must beat ZIP");
}

#[test]
fn later_archive_wins_over_earlier_archive() {
    let dir = TempDir::new("vfszip_archive_priority");
    dir.create_zip("low.zip", &[("shared.txt", b"low")]);
    dir.create_zip("high.zip", &[("shared.txt", b"high")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["low.zip", "high.zip"]));
    let mut buf = Vec::new();
    std::io::Read::read_to_end(
        &mut vfs.get_file("shared.txt").unwrap().open().unwrap(),
        &mut buf,
    )
    .unwrap();

    assert_eq!(buf, b"high");
}

// ---- Flags ----

#[test]
fn zip_entry_is_archive_not_loose() {
    let dir = TempDir::new("vfszip_flag");
    dir.create_zip("data.zip", &[("meshes/cube.nif", b"")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let file = vfs.get_file("meshes/cube.nif").unwrap();

    assert!(file.is_archive());
    assert!(!file.is_loose());
}

#[test]
fn zip_entry_parent_archive_name_matches_zip_filename() {
    let dir = TempDir::new("vfszip_archivename");
    dir.create_zip("mymod.zip", &[("icons/sword.dds", b"")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["mymod.zip"]));
    let file = vfs.get_file("icons/sword.dds").unwrap();

    assert_eq!(file.parent_archive_name().unwrap(), "mymod.zip");
}

// ---- Normalization ----

#[test]
fn zip_entry_case_insensitive_lookup() {
    let dir = TempDir::new("vfszip_case");
    dir.create_zip("data.zip", &[("textures/landscape/foo.dds", b"")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

    assert!(vfs.get_file("textures/landscape/foo.dds").is_some());
    assert!(vfs.get_file("Textures/Landscape/Foo.DDS").is_some());
    assert!(vfs.get_file("TEXTURES\\LANDSCAPE\\FOO.DDS").is_some());
}

#[test]
fn zip_entry_uppercase_name_normalized_to_lowercase_key() {
    let dir = TempDir::new("vfszip_norm");
    // ZIPs from Windows tooling often have uppercase entry names.
    dir.create_zip("data.zip", &[("Meshes/Actors/XBase.NIF", b"nif_data")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

    // Key must be lowercase after normalization
    assert!(vfs.get_file("meshes/actors/xbase.nif").is_some());

    // Content must still be readable despite the normalized lookup key
    let mut buf = Vec::new();
    std::io::Read::read_to_end(
        &mut vfs
            .get_file("meshes/actors/xbase.nif")
            .unwrap()
            .open()
            .unwrap(),
        &mut buf,
    )
    .unwrap();
    assert_eq!(buf, b"nif_data");
}

// ---- PK3 ----

#[test]
fn pk3_extension_treated_as_zip() {
    let dir = TempDir::new("vfszip_pk3");
    dir.create_zip("pak0.pk3", &[("sound/ambient/wind.wav", b"wave_data")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["pak0.pk3"]));

    let file = vfs.get_file("sound/ambient/wind.wav").unwrap();
    assert!(file.is_archive());
    assert_eq!(file.parent_archive_name().unwrap(), "pak0.pk3");
}

// ---- Tree ----

#[test]
fn zip_entries_appear_in_tree() {
    let dir = TempDir::new("vfszip_tree");
    dir.create_zip("data.zip", &[("textures/sky.dds", b"")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let tree = vfs.tree(true);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();

    assert!(
        tree_contains_file(root, "sky.dds"),
        "ZIP entry should appear in tree"
    );
}

#[test]
fn zip_entries_from_tree_are_openable() {
    let dir = TempDir::new("vfszip_tree_openable");
    dir.create_zip("data.zip", &[("textures/sky.dds", b"sky")]);

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let tree = vfs.tree(true);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();

    let file = find_tree_file(root, "sky.dds").expect("tree should include zip file");
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
    assert_eq!(buf, b"sky");
}
