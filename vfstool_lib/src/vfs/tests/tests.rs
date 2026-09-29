use super::*;
use crate::CollapseOptions;
use dream_archive::{Ba2Builder, Tes3BsaBuilder, Tes4BsaBuilder, bsa::tes4::ArchiveTypes};
use std::{
    fs,
    path::{Path, PathBuf},
};

const TEST_DATA: &[&str] = &[
    "file1.txt",
    "file2.txt",
    "file3.txt",
    "file4.txt",
    "file5.txt",
    "file6.txt",
];

const TEST_STRING: &str = "Act IV, Scene III, continued

Lifts-Her-Tail
Certainly not, kind sir! I am here but to clean your chambers.

Crantius Colto
Is that all you have come here for, little one? My chambers?

Lifts-Her-Tail
I have no idea what it is you imply, master. I am but a poor Argonian maid.

Crantius Colto
So you are, my dumpling. And a good one at that. Such strong legs and shapely tail.

Lifts-Her-Tail
You embarrass me, sir!

Crantius Colto
Fear not. You are safe here with me.

Lifts-Her-Tail
I must finish my cleaning, sir. The mistress will have my head if I do not!

Crantius Colto
Cleaning, eh? I have something for you. Here, polish my spear.

Lifts-Her-Tail
But it is huge! It could take me all night!

Crantius Colto
Plenty of time, my sweet. Plenty of time.

END OF ACT IV, SCENE III";

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

    fn child(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let target = self.child(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&target, data).unwrap();
        target
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_files(dir: &Path, files: &[&str]) {
    fs::create_dir_all(dir).unwrap();
    for file in files {
        let file_path = dir.join(file);
        fs::write(file_path, TEST_STRING).unwrap();
    }
}

#[test]
fn test_vfs_from_directories() {
    let temp = TempDir::new("vfs_beth_archive_priority");
    let archive_dir = temp.child("archives");
    fs::create_dir_all(&archive_dir).unwrap();

    let (dir1, dir2, dir3) = create_test_dirs_and_files(&temp);

    let bsa1 = create_tes3_bsa_archive(&archive_dir, "archive1.bsa", &TEST_DATA[0..6]);
    let bsa2 = create_tes3_bsa_archive(&archive_dir, "archive2.bsa", &TEST_DATA[0..5]);
    let bsa3 = create_tes3_bsa_archive(&archive_dir, "archive3.bsa", &TEST_DATA[0..4]);

    let search_dirs = vec![archive_dir, dir1.clone(), dir2.clone(), dir3.clone()];
    let archive_list = vec!["archive1.bsa", "archive2.bsa", "archive3.bsa"];

    let vfs = VFS::from_directories(search_dirs, Some(archive_list));

    verify_file_locations(&vfs, &bsa1, &bsa2, &bsa3, &dir1, &dir2, &dir3);
}

#[test]
fn generated_bethesda_archive_formats_are_readable() {
    let dir = TempDir::new("vfs_beth_archive_formats");
    create_tes3_bsa_archive(
        dir.path(),
        "morrowind.bsa",
        &["Meshes/Foo.NIF", "Textures/Foo.DDS"],
    );
    create_tes4_bsa_archive(dir.path(), "skyrim.bsa");
    create_ba2_archive(dir.path(), "fallout.ba2");

    let vfs = VFS::from_directories(
        vec![dir.path()],
        Some(vec!["morrowind.bsa", "skyrim.bsa", "fallout.ba2"]),
    );

    assert_archive_entry(&vfs, "meshes/foo.nif", b"Meshes/Foo.NIF");
    assert_archive_entry(&vfs, "sound/fx/door.wav", b"tes4 door sound");
    assert_archive_entry(&vfs, "interface/main.swf", b"ba2 interface payload");
}

#[test]
fn archive_entries_are_named_by_the_last_component_of_their_entry_path() {
    let dir = TempDir::new("vfs_archive_entry_file_name");
    create_tes3_bsa_archive(dir.path(), "morrowind.bsa", &["Meshes/X/Ex_Door.NIF"]);
    create_ba2_archive(dir.path(), "fallout.ba2");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["morrowind.bsa", "fallout.ba2"]));
    let door = vfs.get_file("meshes/x/ex_door.nif").unwrap();
    // Morrowind's BSAs store `meshes\x\ex_door.nif`: lowercase, with `\` between folders.
    assert_eq!(door.file_name(), Some(std::ffi::OsStr::new("ex_door.nif")));
    assert_eq!(door.file_stem(), Some(std::ffi::OsStr::new("ex_door")));
    let swf = vfs.get_file("interface/main.swf").unwrap();
    assert_eq!(swf.file_name(), Some(std::ffi::OsStr::new("main.swf")));

    let tree = vfs.tree(true);
    let x = &tree[Path::new("Data Files")].subdirs[Path::new("morrowind.bsa")].subdirs
        [Path::new("meshes")]
    .subdirs[Path::new("x")];
    assert_eq!(
        x.files[0].file_name(),
        Some(std::ffi::OsStr::new("ex_door.nif"))
    );
}

#[test]
fn ba2_archive_entry_loses_to_loose_file() {
    let dir = TempDir::new("vfs_ba2_loose_priority");
    create_ba2_archive(dir.path(), "fallout.ba2");
    let loose = dir.write("interface/main.swf", b"loose interface payload");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["fallout.ba2"]));
    let file = vfs.get_file("interface/main.swf").unwrap();

    assert!(file.is_loose());
    assert_eq!(file.path(), loose);
}

#[test]
fn collapse_extracts_fallout3_bsa_archive_entry() {
    let dir = TempDir::new("vfs_fallout3_collapse_archive");
    create_fallout3_bsa_archive(dir.path(), "Fallout - Meshes.bsa");
    let out = TempDir::new("vfs_fallout3_collapse_archive_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["Fallout - Meshes.bsa"]));
    let file = vfs.get_file("meshes/test.nif").unwrap();
    assert!(file.is_archive());

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
        fs::read(out.path().join("meshes/test.nif")).unwrap(),
        b"fallout3 mesh payload"
    );
    assert!(!out.path().join("Fallout - Meshes.bsa").exists());
}

#[test]
fn collapse_reports_the_archive_entries_it_leaves_out() {
    let dir = TempDir::new("vfs_collapse_report_archive_entries");
    let archive = create_fallout3_bsa_archive(dir.path(), "Fallout - Meshes.bsa");
    let out = TempDir::new("vfs_collapse_report_archive_entries_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["Fallout - Meshes.bsa"]));
    let report = vfs
        .collapse_into(
            out.path(),
            &CollapseOptions {
                allow_copying: true,
                extract_archives: false,
                use_symlinks: false,
            },
        )
        .unwrap();

    assert_eq!(report.written, 1, "the archive file itself is linked");
    assert!(
        matches!(
            report.skipped.as_slice(),
            [crate::MaterializationSkip::ArchiveEntry { key, archive: path }]
                if key == Path::new("meshes/test.nif") && *path == archive
        ),
        "{:?}",
        report.skipped
    );
    assert!(!out.path().join("meshes/test.nif").exists());
}

#[test]
fn collapse_prefers_loose_file_over_fallout3_bsa_archive_entry() {
    let dir = TempDir::new("vfs_fallout3_collapse_loose_priority");
    create_fallout3_bsa_archive(dir.path(), "Fallout - Meshes.bsa");
    dir.write("meshes/test.nif", b"loose mesh payload");
    let out = TempDir::new("vfs_fallout3_collapse_loose_priority_out");

    let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["Fallout - Meshes.bsa"]));
    let file = vfs.get_file("meshes/test.nif").unwrap();
    assert!(file.is_loose());

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
        fs::read(out.path().join("meshes/test.nif")).unwrap(),
        b"loose mesh payload"
    );
    assert!(!out.path().join("Fallout - Meshes.bsa").exists());
}

fn create_test_dirs_and_files(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let dir1 = temp.child("dir1");
    let dir2 = temp.child("dir2");
    let dir3 = temp.child("dir3");

    create_files(&dir1, &TEST_DATA[0..3]);
    create_files(&dir2, &TEST_DATA[0..2]);
    create_files(&dir3, &TEST_DATA[0..1]);

    (dir1, dir2, dir3)
}

fn create_tes3_bsa_archive(archive_dir: &Path, archive_name: &str, data: &[&str]) -> PathBuf {
    let archive_path = archive_dir.join(archive_name);
    let mut builder = Tes3BsaBuilder::new();
    for path in data {
        builder.add_bytes(*path, path.as_bytes()).unwrap();
    }
    builder.write_path(&archive_path).unwrap();
    archive_path
}

fn create_tes4_bsa_archive(archive_dir: &Path, archive_name: &str) -> PathBuf {
    let archive_path = archive_dir.join(archive_name);
    let mut builder = Tes4BsaBuilder::skyrim_le();
    builder
        .add_bytes("sound/fx/door.wav", b"tes4 door sound")
        .unwrap();
    builder
        .add_bytes("textures/landscape/rock.dds", b"tes4 texture payload")
        .unwrap();
    builder.write_path(&archive_path).unwrap();
    archive_path
}

fn create_fallout3_bsa_archive(archive_dir: &Path, archive_name: &str) -> PathBuf {
    let archive_path = archive_dir.join(archive_name);
    let mut builder = Tes4BsaBuilder::fallout3();
    builder.set_archive_types(ArchiveTypes::MESHES);
    builder
        .add_bytes("meshes/test.nif", b"fallout3 mesh payload")
        .unwrap();
    builder.write_path(&archive_path).unwrap();
    archive_path
}

fn create_ba2_archive(archive_dir: &Path, archive_name: &str) -> PathBuf {
    let archive_path = archive_dir.join(archive_name);
    let mut builder = Ba2Builder::new();
    builder
        .add_bytes("interface/main.swf", b"ba2 interface payload")
        .unwrap();
    builder
        .add_bytes("scripts/example.pex", b"ba2 script payload")
        .unwrap();
    builder.write_path(&archive_path).unwrap();
    archive_path
}

fn assert_archive_entry(vfs: &VFS, key: &str, expected: &[u8]) {
    let file = vfs.get_file(key).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file.open().unwrap(), &mut bytes).unwrap();
    assert_eq!(bytes, expected);
    assert!(file.is_archive());
}

fn verify_file_locations(
    vfs: &VFS,
    bsa1: &Path,
    bsa2: &Path,
    bsa3: &Path,
    dir1: &Path,
    dir2: &Path,
    dir3: &Path,
) {
    assert_eq!(
        vfs.get_file("file6.txt")
            .unwrap()
            .parent_archive_path()
            .unwrap(),
        bsa1.to_str().unwrap()
    );

    assert_eq!(
        vfs.get_file("file5.txt")
            .unwrap()
            .parent_archive_path()
            .unwrap(),
        bsa2.to_str().unwrap()
    );

    assert_eq!(
        vfs.get_file("file4.txt")
            .unwrap()
            .parent_archive_path()
            .unwrap(),
        bsa3.to_str().unwrap()
    );

    assert_eq!(
        vfs.get_file("file3.txt").unwrap().path(),
        dir1.join("file3.txt")
    );

    assert_eq!(
        vfs.get_file("file2.txt").unwrap().path(),
        dir2.join("file2.txt")
    );

    assert_eq!(
        vfs.get_file("file1.txt").unwrap().path(),
        dir3.join("file1.txt")
    );
}
