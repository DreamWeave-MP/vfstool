// SPDX-License-Identifier: GPL-3.0-only
mod read {
    use super::super::VfsFile;
    use crate::normalize_host_path;
    use std::{
        fs::{File, OpenOptions, create_dir, metadata, remove_dir_all, remove_file},
        io::{Read, Write},
        path::PathBuf,
        sync::Arc,
        thread,
    };

    const TEST_DATA: &str = "Act IV, Scene III, continued

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

    /// The `VFSFile` itself is *not* responsible for normalization
    /// It contains a reference to the real path, and some helpers to interact with it
    /// Its parent struct, `VFSFiles`, uses the normalized path as a `HashMap` key to refer to the
    /// `VFSFile`
    /// Thus, we should ensure that the path contained in the `VFSFile` is not already normalized
    /// but instead refers to the literal path on the user's system
    #[test]
    fn path_must_not_be_normalized() {
        let test_dir = PathBuf::from("SpOnGeBoBcAsEfIlE");
        let test_path = test_dir.join("wHoOpSyDoOpSy.EsM");

        if metadata(&test_dir).is_err() {
            let path = create_dir(test_dir.clone());
            assert!(
                path.is_ok(),
                "{}",
                format!(
                    "CRITICAL TEST FAILURE: COULD NOT CREATE TEST DIRECTORY: {}!",
                    path.unwrap_err()
                ),
            );
        }

        let _ = File::create(&test_path);
        let vfs_file = VfsFile::from(&test_path);
        let fd = vfs_file.open();

        assert!(fd.is_ok(), "TEST FAILURE: COULD NOT OPEN VFS FILE!");

        assert_ne!(normalize_host_path(&test_path), vfs_file.path());

        let _ = remove_dir_all(test_dir);
    }

    #[test]
    fn paths_must_match() {
        let path = "path/to/some/file";
        let path_buf = PathBuf::from(&path);
        let vfs_file = VfsFile::from(path);
        assert!(&path_buf.eq(vfs_file.path()));
    }

    #[test]
    fn open_existing_file() {
        let test_path = "test_file.txt";
        let _ = File::create(test_path);

        let vfs_file = VfsFile::from(test_path);

        let fd = vfs_file.open();
        assert!(fd.is_ok(), "Opening an existing file should succeed");
        let _ = remove_file(vfs_file.path());
    }

    #[test]
    fn open_non_existing_file() {
        let bad_path = "non_existent_file";
        let file = VfsFile::from(bad_path);

        let fd = file.open();
        assert!(fd.is_err(), "Opening a non-existent file should fail");
    }

    #[test]
    fn open_loose_file_with_weird_chars() -> std::io::Result<()> {
        #[cfg(windows)]
        let test_path =
            "##$$&&&%%&^^^^!!!!!0)))(((()()[[[}}}}}}}{{{{[[[[]]]]}]]]))@@&^^^^!!!___++_==_----.txt";
        #[cfg(not(windows))]
        let test_path = "##$$&&&%%&***^^^^!!!!!0)))(((()()[[[}}}}}}}{{{{[[[[]]]]}]]]))@@&****(&^^^!!!___++_==_----.txt";

        let mut fd = File::create(test_path)?;

        write!(fd, "{TEST_DATA}")?;

        let vfs_file = VfsFile::from(test_path);

        let mut reader = vfs_file.open()?;

        let mut data_buf = String::new();
        let _written = reader.read_to_string(&mut data_buf);

        assert_eq!(data_buf, TEST_DATA);

        remove_file(vfs_file.path())?;

        Ok(())
    }

    #[test]
    fn test_concurrent_reading() {
        let path_str = "test.txt";
        let mut test_file_content = File::create(path_str).unwrap();
        let _ = write!(test_file_content, "{TEST_DATA}");

        let vfs_file = Arc::new(VfsFile::from(path_str));

        vfs_file.open().expect("File should open");

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let vfs_clone = Arc::clone(&vfs_file);
                thread::spawn(move || {
                    let result = vfs_clone.open();
                    assert!(result.is_ok(), "Read should succeed");

                    let mut result_data = String::new();
                    let _ = result.unwrap().read_to_string(&mut result_data);

                    assert_eq!(result_data, TEST_DATA);
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let _ = remove_file(PathBuf::from(path_str));
    }

    /// The OS generally handles concurrent writes, so not much special needs done here
    /// But do note that later iterations of this design won't implement writes this way
    #[test]
    fn test_concurrent_writing() {
        let path_str = "test_write.txt";

        let _ = File::create(path_str).unwrap();

        let vfs_file = Arc::new(VfsFile::from(path_str));

        vfs_file.open().expect("File should open");

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let vfs_clone = Arc::clone(&vfs_file);
                thread::spawn(move || {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .open(vfs_clone.path())
                        .expect("File should be openable in thread!");

                    let write_result = file.write_all(TEST_DATA.as_bytes());

                    assert!(
                        write_result.is_ok(),
                        "Write operations are not natively thread-safe {}!",
                        write_result.unwrap_err()
                    );
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let _ = remove_file(PathBuf::from(path_str));
    }

    /// This usage isn't really necessary, as the OS will handle sequencing of read and write ops
    /// However, if explicit sequencing is required, this is the way to do it
    #[test]
    fn test_concurrent_writing_with_rwlock() {
        let path_str = "test_write_safe.txt";

        let _ = File::create(path_str).expect("Failed to create test file"); // Ensure the file exists

        let vfs_file = Arc::new(VfsFile::from(path_str));
        let file_lock = Arc::new(std::sync::RwLock::new(())); // Lock for write access

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let vfs_clone = Arc::clone(&vfs_file);
                let lock_clone = Arc::clone(&file_lock);

                thread::spawn(move || {
                    let _guard = lock_clone.write().expect("Write lock should succeed");

                    let mut file = match std::fs::OpenOptions::new()
                        .write(true)
                        .open(vfs_clone.path())
                    {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("Thread {i} failed to open file: {e}");
                            return;
                        }
                    };

                    let result = file.write_all(TEST_DATA.as_bytes());
                    assert!(
                        result.is_ok(),
                        "Thread {} failed to write: {}",
                        i,
                        result.unwrap_err()
                    );
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let _ = remove_file(PathBuf::from(path_str));
    }
}
