use std::collections::BTreeMap;
use std::fs::{File as StdFile, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// Define a new trait that combines Read and Seek
trait ReadSeek: Read + Seek {}

// Explicitly implement the ReadSeek trait for std::fs::File
impl ReadSeek for StdFile {}

// This trait mimics the interface of OpenMW's `File`
trait File {
    fn open(&self) -> std::io::Result<Box<dyn ReadSeek>>;
    fn get_path(&self) -> &Path;
}

// Struct representing a file in the VFS
struct VfsFile {
    path: PathBuf,
}

impl VfsFile {
    fn new(path: PathBuf) -> Self {
        VfsFile { path }
    }
}

impl File for VfsFile {
    fn open(&self) -> std::io::Result<Box<dyn ReadSeek>> {
        let file = StdFile::open(&self.path)?;
        Ok(Box::new(file))
    }

    fn get_path(&self) -> &Path {
        &self.path
    }
}

// Function to normalize paths
fn normalize_path(path: &str) -> PathBuf {
    path.to_lowercase().replace("\\", "/").into()
}

fn main() {
    // A map from normalized paths to `File` objects (in this case, `VfsFile` objects)
    let mut file_map: BTreeMap<PathBuf, Box<dyn File>> = BTreeMap::new();

    // Example files
    let file1 = VfsFile::new(normalize_path("textures/armor.dds"));
    let file2 = VfsFile::new(normalize_path("sounds/footstep.wav"));

    // Insert into the map
    file_map.insert(file1.get_path().to_path_buf(), Box::new(file1));
    file_map.insert(file2.get_path().to_path_buf(), Box::new(file2));

    // Perform a lookup
    let query_path = normalize_path("textures/armor.dds");
    if let Some(file) = file_map.get(&query_path) {
        println!("Found file: {}", file.get_path().display());
        // Open the file
        let mut file_stream = file.open().expect("Failed to open file");
        let mut contents = String::new();
        file_stream
            .read_to_string(&mut contents)
            .expect("Failed to read file");
        println!("File contents: {}", contents);
    } else {
        println!("File not found.");
    }
}
