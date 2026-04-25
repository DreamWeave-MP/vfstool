use super::*;
use ba2::tes3::{Archive, ArchiveKey, File};
use std::fs;
use std::path::{Path, PathBuf};

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

fn create_files(dir: &PathBuf, files: &[&str]) {
    fs::create_dir_all(dir).unwrap();
    for file in files {
        let file_path = dir.join(file);
        fs::write(file_path, TEST_STRING).unwrap();
    }
}

#[test]
fn test_vfs_from_directories() {
    let temp_path = std::env::current_dir().unwrap();
    let archive_dir = temp_path.join("archives");

    fs::create_dir_all(&archive_dir).unwrap();

    // Create directories and files
    let (dir1, dir2, dir3) = create_test_dirs_and_files(&temp_path);

    // Create BSA archives
    let bsa1 = create_bsa_archive(&archive_dir, "archive1.bsa", &TEST_DATA[0..6]);
    let bsa2 = create_bsa_archive(&archive_dir, "archive2.bsa", &TEST_DATA[0..5]);
    let bsa3 = create_bsa_archive(&archive_dir, "archive3.bsa", &TEST_DATA[0..4]);

    // Construct VFS
    let search_dirs = vec![
        archive_dir.clone(),
        dir1.clone(),
        dir2.clone(),
        dir3.clone(),
    ];
    let archive_list = vec!["archive1.bsa", "archive2.bsa", "archive3.bsa"];

    let vfs = VFS::from_directories(search_dirs.clone(), Some(archive_list));

    // Verify file locations
    verify_file_locations(&vfs, &bsa1, &bsa2, &bsa3, &dir1, &dir2, &dir3);

    // Clean up test files and directories
    clean_up_test_files(&search_dirs);
}

fn create_test_dirs_and_files(temp_path: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let dir1 = temp_path.join("dir1");
    let dir2 = temp_path.join("dir2");
    let dir3 = temp_path.join("dir3");

    create_files(&dir1, &TEST_DATA[0..3]); // file1.txt, file2.txt, file3.txt
    create_files(&dir2, &TEST_DATA[0..2]); // file1.txt, file2.txt
    create_files(&dir3, &TEST_DATA[0..1]); // file1.txt
    create_files(&temp_path.to_path_buf(), TEST_DATA);

    (dir1, dir2, dir3)
}

fn create_bsa_archive(archive_dir: &Path, archive_name: &str, data: &[&str]) -> PathBuf {
    let archive_path = archive_dir.join(archive_name);
    let archive: Archive = data
        .iter()
        .map(|s| {
            let key: ArchiveKey = s.to_string().into();
            let file: File = File::from(s.as_bytes());
            (key, file)
        })
        .collect();
    let mut dst = fs::File::create(&archive_path).unwrap();
    archive.write(&mut dst).unwrap();
    archive_path
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
        vfs.file_map
            .get(&PathBuf::from("file6.txt"))
            .unwrap()
            .parent_archive_path()
            .unwrap(),
        bsa1.to_str().unwrap()
    );

    assert_eq!(
        vfs.file_map
            .get(&PathBuf::from("file5.txt"))
            .unwrap()
            .parent_archive_path()
            .unwrap(),
        bsa2.to_str().unwrap()
    );

    assert_eq!(
        vfs.file_map
            .get(&PathBuf::from("file4.txt"))
            .unwrap()
            .parent_archive_path()
            .unwrap(),
        bsa3.to_str().unwrap()
    );

    assert_eq!(
        vfs.file_map
            .get(&PathBuf::from("file3.txt"))
            .unwrap()
            .path(),
        dir1.join("file3.txt")
    );

    assert_eq!(
        vfs.file_map
            .get(&PathBuf::from("file2.txt"))
            .unwrap()
            .path(),
        dir2.join("file2.txt")
    );

    assert_eq!(
        vfs.file_map
            .get(&PathBuf::from("file1.txt"))
            .unwrap()
            .path(),
        dir3.join("file1.txt")
    );
}

fn clean_up_test_files(search_dirs: &[PathBuf]) {
    for dir in search_dirs {
        fs::remove_dir_all(dir).unwrap();
    }
    for test_file in TEST_DATA {
        fs::remove_file(test_file).unwrap();
    }
}
