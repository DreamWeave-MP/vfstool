use std::collections::BTreeMap;
use std::fs::{File as StdFile, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

struct VFS {
    data_directories: Vec<PathBuf>,
    file_map: BTreeMap<PathBuf, Arc<VfsFile>>,
}

impl VFS {
    pub fn new() -> Self {
        Self {
            data_directories: Vec::new(),
            file_map: BTreeMap::new(),
        }
    }

    // Function to normalize paths
    fn normalize_path(path: &str) -> PathBuf {
        path.to_lowercase().replace("\\", "/").into()
    }

    /// Looks up a file in the VFS after normalizing the path
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> Option<&Arc<VfsFile>> {
        let normalized_path = Self::normalize_path(&path.as_ref().to_string_lossy());
        self.file_map.get(&normalized_path)
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl Iterator<Item = (String, &Arc<VfsFile>)> {
        let normalized_substring = Self::normalize_path(substring.as_ref());

        self.file_map.iter().filter_map(move |(path, file)| {
            if path
                .to_string_lossy()
                .contains(&*normalized_substring.to_string_lossy())
            {
                Some((path.to_string_lossy().to_string(), file))
            } else {
                None
            }
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl Iterator<Item = (String, &Arc<VfsFile>)> {
        let normalized_prefix = Self::normalize_path(&prefix.as_ref().to_string_lossy());

        self.file_map.iter().filter_map(move |(path, file)| {
            if path.starts_with(&normalized_prefix) {
                Some((path.to_string_lossy().to_string(), file))
            } else {
                None
            }
        })
    }

    pub fn add_files_from_directory(
        &mut self,
        base_dir: &Path,
        search_dir: Option<&Path>,
    ) -> std::io::Result<()> {
        let search_dir = match search_dir {
            None => base_dir,
            Some(dir) => dir,
        };
        let entries = match std::fs::read_dir(search_dir) {
            Ok(entries) => entries,
            Err(error) => {
                eprintln!(
                    "WARNING: Could not read directory '{}': {}",
                    search_dir.display(),
                    error
                );
                return Ok(());
            }
        };

        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Err(error) = self.add_files_from_directory(&base_dir, Some(&path)) {
                            eprintln!(
                                "WARNING: Error occurred recursively adding child directory to VFS: {}",
                                error
                            );
                        }
                    } else if path.is_file() {
                        // Get a relative path from the VFS root
                        let relative_path = path.strip_prefix(base_dir).unwrap_or(&path);

                        // Normalize and store in file_map
                        let normalized_path =
                            Self::normalize_path(&relative_path.to_string_lossy());
                        let vfs_file = VfsFile::new(path);
                        self.file_map.insert(normalized_path, Arc::new(vfs_file));
                    }
                }
                Err(error) => {
                    eprintln!(
                        "WARNING: Directory entry could not be read by the VFS: {}",
                        error
                    );
                }
            }
        }

        Ok(())
    }
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut tree: BTreeMap<String, Vec<String>> = BTreeMap::new();

        // Organize paths into hierarchical groups
        for path in self.file_map.keys() {
            let mut components: Vec<String> = path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();

            if let Some(file) = components.pop() {
                let dir = if components.is_empty() {
                    "/".to_string()
                } else {
                    components.join("/")
                };
                tree.entry(dir).or_default().push(file);
            }
        }

        // Print hierarchy
        writeln!(f, "/")?;
        for (dir, files) in &tree {
            if dir != "/" {
                writeln!(f, "├── {}/", dir)?;
            }
            for file in files {
                writeln!(f, "│   ├── {}", file)?;
            }
        }
        Ok(())
    }
}

fn main() {
    let mut vfs = VFS::new();
    let mw_dir = PathBuf::from("/home/sk3shun-8/BethGames/Morrowind/Data Files/");
    vfs.add_files_from_directory(&mw_dir, None)
        .expect("VFS Construction failed!");
    // println!("{}", vfs);

    // Perform a lookup
    if let Some(file) = vfs.get_file(&"music/explore/mx_ExPlOrE_2.mp3") {
        println!("Found file: {}", file.get_path().display());
        // Open the file
        let mut file_stream = file.open().expect("Failed to open file");
        let mut contents = Vec::new();
        file_stream
            .read_to_end(&mut contents)
            .expect("Failed to read file");
        println!("File contents: {}", contents.len());
    } else {
        println!("File not found.");
    }

    let prefix = "music/explore";
    for (path, file) in vfs.paths_with(prefix) {
        let mut fd = file.open().expect("");
        let mut contents = Vec::new();
        fd.read_to_end(&mut contents).expect("");
        println!(
            "Found prefix-matching file in VFS: {} of size {}",
            path,
            contents.len()
        );
    }

    let prefix = "explore/";
    for (path, file) in vfs.paths_matching(prefix) {
        let mut fd = file.open().expect("");
        let mut contents = Vec::new();
        fd.read_to_end(&mut contents).expect("");
        println!(
            "Found fuzzy matching file in VFS: {} of size {}",
            path,
            contents.len()
        );
    }
}
