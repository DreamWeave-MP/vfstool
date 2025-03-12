use bsatoollib::BSAFile;
use std::{
    fs::File as StdFile,
    io::{self, Cursor, Error, ErrorKind, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
pub struct ArchiveReference {
    path: PathBuf,
    parent_archive: Arc<BSAFile>,
}

#[derive(Debug)]
pub enum FileType {
    Archive(ArchiveReference),
    Loose(PathBuf),
}

/// Represents a file within the Virtual File System (VFS).
///
/// This struct encapsulates a file that exists in the real filesystem but is managed
/// within the VFS. Each `VfsFile` maintains a reference to its original, **non-normalized**
/// path to ensure correct file operations. Paths should only be normalized when **retrieved**,
/// not when constructing the file, as normalization may affect OS file resolution.
///
/// Files in the VFS should be **unique** and stored in a HashMap inside the `VFS` struct.
/// They are typically wrapped in `Arc<VfsFile>` for safe concurrent access.
#[derive(Debug)]
pub struct VfsFile {
    file: FileType,
}

impl Default for VfsFile {
    fn default() -> Self {
        Self {
            file: FileType::Loose(PathBuf::default()),
        }
    }
}

impl VfsFile {
    /// Creates a new `VfsFile` instance with the given `path`.
    ///
    /// # Arguments
    ///
    /// * `path` - An owned `PathBuf` representing the file's location on disk.
    ///
    /// # Notes
    ///
    /// - Paths **must not be normalized** at creation time to avoid potential file lookup issues.
    /// - VfsFile does not, itself, verify that the provided path exists at creation time
    /// this responsibility is left up to its constructor (typically, the VFS struct)
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool::VfsFile;
    ///
    /// let path = "C:\\Morrowind\\Data Files\\Morrowind.esm";
    ///
    /// let file = VfsFile::from(path);
    /// assert_eq!(file.path().to_str(), Some(path));
    /// ```
    pub fn from<P: AsRef<Path>>(path: P) -> Self {
        VfsFile {
            file: FileType::Loose(path.as_ref().to_path_buf()),
        }
    }

    pub fn from_archive<S: AsRef<str>>(path: S, parent_archive: Arc<BSAFile>) -> Self {
        let path = PathBuf::from(path.as_ref());
        VfsFile {
            file: FileType::Archive(ArchiveReference {
                path,
                parent_archive,
            }),
        }
    }

    pub fn is_loose(&self) -> bool {
        match self.file {
            FileType::Archive(_) => false,
            FileType::Loose(_) => true,
        }
    }

    pub fn is_archive(&self) -> bool {
        match self.file {
            FileType::Archive(_) => true,
            FileType::Loose(_) => false,
        }
    }

    pub fn parent_archive_path(&self) -> Result<String, Error> {
        match &self.file {
            FileType::Archive(archive_ref) => {
                Ok(archive_ref.parent_archive.file_name().to_string())
            }
            FileType::Loose(_) => Err(Error::new(
                ErrorKind::InvalidData,
                "Loose files may not return an archive reference!",
            )),
        }
    }

    pub fn parent_archive_name(&self) -> Result<String, Error> {
        match &self.file {
            FileType::Archive(archive_ref) => {
                let name = archive_ref.parent_archive.file_name();
                let split = name.rsplit("/").next().unwrap().to_string();

                Ok(split)
            }
            FileType::Loose(_) => Err(Error::new(
                ErrorKind::InvalidData,
                "Loose files may not return an archive reference!",
            )),
        }
    }

    pub fn parent_archive_handle(&self) -> Result<Arc<BSAFile>, Error> {
        match &self.file {
            FileType::Archive(archive_ref) => Ok(Arc::clone(&archive_ref.parent_archive)),
            FileType::Loose(_) => Err(Error::new(
                ErrorKind::InvalidData,
                "Loose files may not return an archive reference!",
            )),
        }
    }

    /// Opens the file and returns a standard `File` handle.
    ///
    /// # Returns
    ///
    /// * `Ok(StdFile)` - If the file exists and can be opened.
    /// * `Err(io::Error)` - If the file does not exist or cannot be opened.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool::VfsFile;
    ///
    /// let path = "C:\\Some\\Very\\Long\\Path";
    ///
    /// let file = VfsFile::from(path);
    /// let result = file.open();
    ///
    /// assert!(result.is_err());
    /// ```
    pub fn open(&self) -> io::Result<Box<dyn Read>> {
        match &self.file {
            FileType::Loose(path) => {
                let file = StdFile::open(&path)?;
                Ok(Box::new(file))
            }
            FileType::Archive(archive_ref) => {
                let data = archive_ref
                    .parent_archive
                    .get_file(&archive_ref.path.to_string_lossy()[..])
                    .map_err(|err| Error::new(ErrorKind::InvalidData, err.to_string()))?;
                let cursor = Cursor::new(data);
                Ok(Box::new(cursor))
            }
        }
    }

    /// Retrieves the file name (i.e., the last component of the path).
    ///
    /// # Returns
    ///
    /// * `Some(&str)` - If the path contains a valid file name.
    /// * `None` - If the path does not have a file name. This should be a rare exception as any
    /// files typically used *will* have extensions, but it is not necessarily mandatory (eg unix
    /// binaries)
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use vfstool::VfsFile;
    ///
    /// let morrowind_esm = PathBuf::from("C:").join("Morrowind").join("Data
    /// Files").join("Morrowind.esm");
    ///
    /// let file = VfsFile::from(morrowind_esm);
    /// assert_eq!(file.file_name(), Some("Morrowind.esm"));
    /// ```
    pub fn file_name(&self) -> Option<&str> {
        match &self.file {
            // This doesn't actually retrieve the filename, it just normalizes it
            // Now it does retrieve the filename, but wtf
            FileType::Archive(archive_ref) => archive_ref.path.file_name()?.to_str(),
            FileType::Loose(path) => path.file_name()?.to_str(),
        }
    }

    /// Returns the original (non-normalized) path of the file.
    ///
    /// # Returns
    ///
    /// * `&Path` - The path used when creating this `VfsFile`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vfstool::VfsFile;
    /// use std::path::PathBuf;
    ///
    /// let path = "C:\\Morrowind\\Data Files\\Morrowind.esm";
    ///
    /// let file = VfsFile::from(path);
    /// assert_eq!(file.path(), PathBuf::from(path));
    /// ```
    pub fn path(&self) -> &Path {
        match &self.file {
            FileType::Loose(path) => path.as_path(),
            FileType::Archive(archive_ref) => &archive_ref.path,
        }
    }
}

#[cfg(test)]
mod read {
    use super::VfsFile;
    use crate::normalize_path;
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

    /// The VFSFile itself is *not* responsible for normalization
    /// It contains a reference to the real path, and some helpers to interact with it
    /// Its parent struct, VFSFiles, uses the normalized path as a HashMap key to refer to the
    /// VFSFile
    /// Thus, we should ensure that the path contained in the VFSFile is not already normalized
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

        assert_ne!(normalize_path(&test_path), vfs_file.path());

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
        let _ = File::create(&test_path);

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
        let test_path = "##$$&&&%%&***^^^^!!!!!0)))(((()()[[[}}}}}}}{{{{[[[[]]]]}]]]))@@&****(&^^^!!!___++_==_----.txt";

        let mut fd = File::create(&test_path)?;

        write!(fd, "{}", TEST_DATA)?;

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
        let _ = write!(test_file_content, "{}", TEST_DATA);

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
                            eprintln!("Thread {} failed to open file: {}", i, e);
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
