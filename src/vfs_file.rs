use serde::{Serialize, Serializer};

use std::{
    ffi::OsStr,
    fs::File as StdFile,
    io,
    path::{Path, PathBuf},
};

/// Struct representing a file in the VFS
#[derive(Debug)]
pub struct VfsFile {
    /// Refers to the literal path from which a VFSFile was constructed
    /// Private, since it probably will not be normalized beforehand (as that would not
    /// work)
    path: PathBuf,
}

impl VfsFile {
    pub fn new(path: PathBuf) -> Self {
        // Remember, vfsfile entries cannot be constructed *with* the normalized path
        // Calls to open might fail if it's already cleaned up once we ask the OS for it
        // However, if we just give back the normalized path when it's asked for,
        // that's okay
        VfsFile { path }
    }

    fn open(&self) -> io::Result<Box<std::fs::File>> {
        let file = StdFile::open(&self.path)?;
        Ok(Box::new(file))
    }

    pub fn file_name(&self) -> Option<&OsStr> {
        self.path.file_name()
    }

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
        let filename = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(); // Ensure we never panic

        serializer.serialize_str(filename)
    }
}

#[cfg(test)]
mod read {
    use super::VfsFile;
    use std::{
        fs::{File, remove_file},
        path::PathBuf,
    };

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
}

#[cfg(test)]
mod write {
    use crate::VfsFile;
    use std::{collections::BTreeMap, fs};

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

    #[test]
    fn test_serialize_yaml() {
        let vfs_file = VfsFile::new("serialized.yaml".into());
        let serialized = serde_yaml_with_quirks::to_string(&vfs_file);

        assert!(serialized.is_ok(), "Serialization to YAML should succeed");
    }

    #[test]
    fn test_serialize_json() {
        let vfs_file = VfsFile::new("serialized.json".into());
        let serialized = serde_json::to_string_pretty(&vfs_file);

        assert!(serialized.is_ok(), "Serialization to TOML should succeed");

        let _ = fs::remove_file("output.toml"); // Cleanup
    }
}
