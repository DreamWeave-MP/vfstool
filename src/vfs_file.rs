use serde::{Serialize, Serializer};

use std::{
    ffi::OsStr,
    fs::File as StdFile,
    io,
    path::{Path, PathBuf},
};

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
    /// The original path of the file on disk.
    /// This is **not normalized** to ensure that OS-dependent behavior remains valid.
    /// Normalization should be applied only when querying paths.
    path: PathBuf,
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
    /// let file = VfsFile::new(PathBuf::from("C:\\Morrowind\\Data Files\\Morrowind.esm"));
    /// assert_eq!(file.path().to_str(), Some("C:\\Morrowind\\Data Files\\Morrowind.esm"));
    /// ```
    pub fn new(path: PathBuf) -> Self {
        VfsFile { path }
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
    /// use vfstool::VfsFile;
    /// use std::path::PathBuf;
    ///
    /// let file = VfsFile::new(PathBuf::from("C:\\Not\\Morrowind\\Data
    /// Files\\Maybe\\Even\\Oblivion\\Data\\Morrowind.esm"));
    /// let result = file.open();
    ///
    /// assert!(result.is_err()); // Depends on whether file exists
    /// ```
    pub fn open(&self) -> io::Result<StdFile> {
        let file = StdFile::open(&self.path)?;
        Ok(file)
    }

    /// Retrieves the file name (i.e., the last component of the path).
    ///
    /// # Returns
    ///
    /// * `Some(&OsStr)` - If the path contains a valid file name.
    /// * `None` - If the path does not have a file name. This should be a rare exception as any
    /// files typically used *will* have extensions, but it is not necessarily mandatory (eg unix
    /// binaries)
    ///
    /// # Examples
    ///
    /// ```
    /// use vfstool::VfsFile;
    /// use std::{path::PathBuf, ffi::OsStr};
    ///
    /// let morrowind_esm = PathBuf::from("C:").join("Morrowind").join("Data
    /// Files").join("Morrowind.esm");
    ///
    /// let file = VfsFile::new(morrowind_esm);
    /// assert_eq!(file.file_name(), Some(OsStr::new("Morrowind.esm")));
    /// ```
    pub fn file_name(&self) -> Option<&OsStr> {
        self.path.file_name()
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
    /// let file = VfsFile::new(PathBuf::from("C:\\Morrowind\\Data Files\\Morrowind.esm"));
    /// assert_eq!(file.path().to_str(), Some("C:\\Morrowind\\Data Files\\Morrowind.esm"));
    /// ```
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Sentinel VfsFile, representing an invalid path
impl Default for VfsFile {
    fn default() -> Self {
        VfsFile {
            path: PathBuf::new(),
        }
    }
}

impl Serialize for VfsFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let filename = self.path.file_name().and_then(|name| name.to_str());

        match filename {
            None => Err(serde::ser::Error::custom("Failed to get file name!")),
            Some(result) => serializer.serialize_str(result),
        }
    }
}

#[cfg(test)]
mod read {
    use super::VfsFile;
    use crate::normalize_path;
    use std::{
        fs::{File, create_dir, remove_dir_all, remove_file},
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

        if std::fs::metadata(&test_dir).is_err() {
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
        let vfs_file = VfsFile::new(test_path.clone());
        let fd = vfs_file.open();

        assert!(fd.is_ok(), "TEST FAILURE: COULD NOT OPEN VFS FILE!");

        assert_ne!(normalize_path(&test_path), vfs_file.path());

        let _ = remove_dir_all(test_dir);
    }

    #[test]
    fn paths_must_match() {
        let test_path = PathBuf::from("path/to/some/file");
        let vfs_file = VfsFile::new(test_path.clone());
        assert!(&test_path.eq(vfs_file.path()));
    }

    #[test]
    fn open_existing_file() {
        let test_path = PathBuf::from("test_file.txt");
        let _ = File::create(&test_path);

        let vfs_file = VfsFile::new(test_path);

        let fd = vfs_file.open();
        assert!(fd.is_ok(), "Opening an existing file should succeed");
        let _ = remove_file(vfs_file.path);
    }

    #[test]
    fn open_non_existing_file() {
        let bad_path = PathBuf::from("non_existent_file");
        let file = VfsFile::new(bad_path);

        let fd = file.open();
        assert!(fd.is_err(), "Opening a non-existent file should fail");
    }

    #[test]
    fn open_file_with_weird_chars() {
        let test_path = PathBuf::from(
            "##$$&&&%%&***^^^^!!!!!0)))(((()()[[[}}}}}}}{{{{[[[[]]]]}]]]))@@&****(&^^^!!!___++_==_----.txt",
        );
        let _ = File::create(&test_path);

        let vfs_file = VfsFile::new(test_path);

        let fd = vfs_file.open();

        assert!(
            fd.is_ok(),
            "Opening an existing file should succeed: {}",
            fd.unwrap_err()
        );

        let _ = remove_file(vfs_file.path);
    }

    #[test]
    fn test_concurrent_reading() {
        let path_str = "test.txt";
        let mut test_file_content = File::create(path_str).unwrap();
        let _ = write!(test_file_content, "{}", TEST_DATA);

        let vfs_file = Arc::new(VfsFile::new(path_str.into()));

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

    #[test]
    fn test_concurrent_writing() {
        let path_str = "test_write.txt";

        let _ = File::create(path_str).unwrap();

        let vfs_file = Arc::new(VfsFile::new(path_str.into()));

        vfs_file.open().expect("File should open");

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let vfs_clone = Arc::clone(&vfs_file);
                thread::spawn(move || {
                    let mut file = std::fs::OpenOptions::new()
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

        let vfs_file = Arc::new(VfsFile::new(path_str.into()));
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

#[cfg(test)]
mod write {
    use crate::VfsFile;
    use std::collections::BTreeMap;

    /// Raw VFSFiles may not serialize to TOML
    /// as the format requires k/v pairs
    #[test]
    fn serialize_toml() {
        let mut tree = BTreeMap::new();

        let vfs_file = VfsFile::new("serialized.toml".into());
        tree.insert("Some_File", vfs_file);

        let serialized = toml::to_string_pretty(&tree);

        assert!(
            serialized.is_ok(),
            "Serialization to TOML should succeed : {}",
            serialized.unwrap_err().to_string()
        );
    }

    /// Serialize individual files straight to json or yaml
    #[test]
    fn test_serialize_yaml() {
        let vfs_file = VfsFile::new("serialized.yaml".into());
        let serialized = serde_yaml_with_quirks::to_string(&vfs_file);

        assert!(serialized.is_ok(), "Serialization to YAML should succeed");
    }

    /// Serialize individual files straight to json or yaml
    #[test]
    fn test_serialize_json() {
        let name = "serialized.json";
        let vfs_file = VfsFile::new(name.into());
        let serialized = serde_json::to_string_pretty(&vfs_file);

        assert!(serialized.is_ok(), "Serialization to JSON should succeed");
    }
}
