use serde::{Serialize, Serializer};

use std::{
    fs::File as StdFile,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};

// Define a new trait that combines Read and Seek
trait ReadSeek: Read + Seek {}

// Explicitly implement the ReadSeek trait for std::fs::File
impl ReadSeek for StdFile {}

// This trait mimics the interface of OpenMW's `File`
trait File {
    fn open(&self) -> io::Result<Box<dyn ReadSeek>>;
    fn get_path(&self) -> &Path;
}

/// Struct representing a file in the VFS
#[derive(Debug)]
pub struct VfsFile {
    pub path: PathBuf,
}

impl VfsFile {
    pub fn new(path: PathBuf) -> Self {
        VfsFile { path }
    }
}

impl File for VfsFile {
    fn open(&self) -> io::Result<Box<dyn ReadSeek>> {
        let file = StdFile::open(&self.path)?;
        Ok(Box::new(file))
    }

    fn get_path(&self) -> &Path {
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
