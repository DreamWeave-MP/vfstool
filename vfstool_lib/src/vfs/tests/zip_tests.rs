use super::*;
use crate::{CollapseOptions, DirectoryNode, MaterializationAction};
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

    fn create_zip(&self, filename: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        self.create_zip_with_method(filename, entries, zip::CompressionMethod::Stored)
    }

    /// Create a ZIP file at `filename` (relative to this dir) with the given entries.
    fn create_zip_with_method(
        &self,
        filename: &str,
        entries: &[(&str, &[u8])],
        method: zip::CompressionMethod,
    ) -> PathBuf {
        let path = self.0.join(filename);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
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
fn try_from_directories_reports_missing_configured_zip() {
    let dir = TempDir::new("vfszip_missing_strict");

    let Err(err) = VFS::try_from_directories([dir.path()], Some(vec!["missing.zip"])) else {
        panic!("strict construction should reject a missing configured archive");
    };

    assert!(matches!(
        err,
        VfsBuildError::ArchiveNotFound { archive } if archive == "missing.zip"
    ));
}

#[test]
fn try_from_directories_reports_broken_configured_zip() {
    let dir = TempDir::new("vfszip_broken_strict");
    dir.write("data.zip", b"not actually a zip file");

    let Err(err) = VFS::try_from_directories([dir.path()], Some(vec!["data.zip"])) else {
        panic!("strict construction should reject a broken configured archive");
    };

    assert!(
        matches!(err, VfsBuildError::ArchiveLoad { archive, .. } if archive.ends_with("data.zip"))
    );
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
fn deflated_zip_entry_content_readable() {
    let dir = TempDir::new("vfszip_deflated_content");
    dir.create_zip_with_method(
        "data.zip",
        &[("scripts/deflated.lua", b"return 'deflated'")],
        zip::CompressionMethod::Deflated,
    );

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let file = vfs.get_file("scripts/deflated.lua").unwrap();

    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
    assert_eq!(buf, b"return 'deflated'");
}

#[test]
fn lzma_zip_entry_content_readable() {
    let dir = TempDir::new("vfszip_lzma_content");
    // zip 8 can read LZMA with the `lzma` feature, but intentionally does not
    // write it. Use a tiny fixture produced by Python's zipfile module instead
    // of pretending the writer supports what it does not. It complained. It was right.
    dir.write(
        "data.zip",
        &[
            80, 75, 3, 4, 63, 0, 2, 0, 14, 0, 59, 85, 156, 92, 122, 195, 53, 52, 33, 0, 0, 0, 13,
            0, 0, 0, 16, 0, 0, 0, 115, 99, 114, 105, 112, 116, 115, 47, 108, 122, 109, 97, 46, 108,
            117, 97, 9, 4, 5, 0, 93, 0, 0, 128, 0, 0, 57, 25, 74, 240, 49, 180, 69, 174, 28, 147,
            107, 213, 212, 128, 120, 103, 30, 151, 255, 233, 135, 128, 0, 80, 75, 1, 2, 63, 3, 63,
            0, 2, 0, 14, 0, 59, 85, 156, 92, 122, 195, 53, 52, 33, 0, 0, 0, 13, 0, 0, 0, 16, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 1, 0, 0, 0, 0, 115, 99, 114, 105, 112, 116, 115, 47,
            108, 122, 109, 97, 46, 108, 117, 97, 80, 75, 5, 6, 0, 0, 0, 0, 1, 0, 1, 0, 62, 0, 0, 0,
            79, 0, 0, 0, 0, 0,
        ],
    );

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let file = vfs.get_file("scripts/lzma.lua").unwrap();

    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
    assert_eq!(buf, b"return 'lzma'");
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
fn remove_loose_winner_reveals_archive_provider() {
    let dir = TempDir::new("vfszip_reveal_archive_provider");
    dir.create_zip("data.zip", &[("textures/foo.dds", b"archive")]);
    let loose_file = dir.write("textures/foo.dds", b"loose");

    let mut vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let providers = vfs
        .providers_for("textures/foo.dds")
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].source.kind, crate::SourceKind::Archive);
    assert_eq!(providers[1].source.kind, crate::SourceKind::LooseDir);
    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), loose_file);

    let removed = vfs.remove_winner("textures/foo.dds").unwrap();
    assert_eq!(removed.source.kind, crate::SourceKind::LooseDir);
    assert!(vfs.get_file("textures/foo.dds").unwrap().is_archive());
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

#[test]
fn zip_duplicate_normalized_entries_are_reported() {
    let dir = TempDir::new("vfszip_duplicate_normalized_entries");
    let archive = dir.create_zip(
        "data.zip",
        &[
            ("Textures/Foo.DDS", b"upper"),
            ("textures/foo.dds", b"lower"),
        ],
    );

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

    let providers = vfs.provider_records_for("textures/foo.dds");
    assert_eq!(providers.len(), 2);
    let original_paths = providers
        .iter()
        .map(|provider| provider.original_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(original_paths.contains(Path::new("Textures/Foo.DDS")));
    assert!(original_paths.contains(Path::new("textures/foo.dds")));

    let entries = vfs.archive_entries(archive);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.iter().filter(|entry| entry.wins).count(), 1);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.wins)
            .unwrap()
            .original_path,
        PathBuf::from("textures/foo.dds")
    );
    let archive_info = vfs.archives();
    assert_eq!(archive_info.len(), 1);
    assert_eq!(archive_info[0].entry_count, 2);
    assert_eq!(archive_info[0].winning_entry_count, 1);
    let collisions = vfs.case_collisions();
    assert_eq!(collisions.collisions.len(), 1);
    assert_eq!(collisions.collisions[0].providers.len(), 2);
}

#[test]
fn materialization_plan_skips_loose_zip_when_extracting_archive_entries() {
    let dir = TempDir::new("vfszip_materialization_plan_skip_zip");
    dir.create_zip("data.zip", &[("scripts/test.lua", b"return 42")]);
    let out = TempDir::new("vfszip_materialization_plan_skip_zip_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    let plan = vfs.materialization_plan(
        out.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    );

    assert!(plan.issues.is_empty());
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        MaterializationAction::SkipArchiveFile { key, .. } if key == Path::new("data.zip")
    )));
    assert!(plan.actions.iter().any(|action| matches!(
        action,
        MaterializationAction::ExtractArchive { key, .. } if key == Path::new("scripts/test.lua")
    )));
}

#[test]
fn collapse_extract_archives_skips_loose_zip_file() {
    let dir = TempDir::new("vfszip_collapse_skip_zip");
    dir.create_zip("data.zip", &[("scripts/test.lua", b"return 42")]);
    let out = TempDir::new("vfszip_collapse_skip_zip_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
    vfs.collapse_into(
        out.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    )
    .unwrap();

    assert_eq!(
        fs::read(out.path().join("scripts/test.lua")).unwrap(),
        b"return 42"
    );
    assert!(!out.path().join("data.zip").exists());
}

#[test]
fn collapse_extract_archives_skips_loose_pk3_file() {
    let dir = TempDir::new("vfszip_collapse_skip_pk3");
    dir.create_zip("pak0.pk3", &[("sound/ambient/wind.wav", b"wave_data")]);
    let out = TempDir::new("vfszip_collapse_skip_pk3_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["pak0.pk3"]));
    vfs.collapse_into(
        out.path(),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: true,
            use_symlinks: false,
        },
    )
    .unwrap();

    assert_eq!(
        fs::read(out.path().join("sound/ambient/wind.wav")).unwrap(),
        b"wave_data"
    );
    assert!(!out.path().join("pak0.pk3").exists());
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
