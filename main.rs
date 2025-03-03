use rayon::prelude::*;
use walkdir::WalkDir;

use std::{
    collections::BTreeMap,
    fs::File as StdFile,
    io::{Read, Result, Seek},
    ops::Index,
    path::{Path, PathBuf},
    sync::Arc,
};

// Define a new trait that combines Read and Seek
trait ReadSeek: Read + Seek {}

// Explicitly implement the ReadSeek trait for std::fs::File
impl ReadSeek for StdFile {}

// This trait mimics the interface of OpenMW's `File`
trait File {
    fn open(&self) -> Result<Box<dyn ReadSeek>>;
    fn get_path(&self) -> &Path;
}

#[derive(Debug)]
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
    fn open(&self) -> Result<Box<dyn ReadSeek>> {
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

impl PartialEq<VfsFile> for &VfsFile {
    fn eq(&self, other: &VfsFile) -> bool {
        self == other
    }
}

struct VFS {
    file_map: BTreeMap<PathBuf, Arc<VfsFile>>,
}

impl VFS {
    pub fn new() -> Self {
        Self {
            file_map: BTreeMap::new(),
        }
    }

    /// Lowercase path and convert path separators to unix-style
    fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
        let path = path
            .as_ref()
            .to_string_lossy()
            .to_lowercase()
            .replace("\\", "/");
        PathBuf::from(path)
    }

    /// Looks up a file in the VFS after normalizing the path
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> Option<&Arc<VfsFile>> {
        let normalized_path = Self::normalize_path(path.as_ref());
        self.file_map.get(&normalized_path)
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl ParallelIterator<Item = (&Path, &Arc<VfsFile>)> {
        let normalized_substring = Self::normalize_path(substring.as_ref())
            .to_string_lossy()
            .into_owned();

        self.file_map.par_iter().filter_map(move |(path, file)| {
            if path.to_string_lossy().contains(&normalized_substring) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl ParallelIterator<Item = (&Path, &Arc<VfsFile>)> {
        let normalized_prefix = Self::normalize_path(&prefix);

        self.file_map.par_iter().filter_map(move |(path, file)| {
            if path.starts_with(&normalized_prefix) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a Path to a directory, return a vector of tuples containing the VFS objects
    /// NO FUCKING RECURSION
    fn process_directory(
        base_dir: &Path,
        search_dir: Option<&Path>,
    ) -> Vec<(PathBuf, Arc<VfsFile>)> {
        let mut files = Vec::new();
        let search_dir = match search_dir {
            None => base_dir,
            Some(dir) => dir,
        };

        // Stack to hold directories that need processing
        let mut dirs_to_process = vec![search_dir.to_path_buf()];

        while let Some(current_dir) = dirs_to_process.pop() {
            // Read the directory and handle any errors
            let entries = match std::fs::read_dir(&current_dir) {
                Ok(entries) => entries,
                Err(error) => {
                    eprintln!(
                        "WARNING: Could not read directory '{}': {}",
                        current_dir.display(),
                        error
                    );
                    continue;
                }
            };

            // Process the directory entries
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    // Add subdirectories to the stack for later processing
                    dirs_to_process.push(path);
                } else if path.is_file() {
                    // Add the file to the list
                    let relative_path = path.strip_prefix(base_dir).unwrap_or(&path);
                    let normalized_path = Self::normalize_path(&relative_path);
                    let vfs_file = VfsFile::new(path);
                    files.push((normalized_path, Arc::new(vfs_file)));
                }
            }
        }

        files
    }

    /// Given a vector of paths, collects their VFS entries in parallel and then applies them in sequence
    /// 1: Get an indexed list of all folders with entries generated, in parallel
    /// 2: Sort them according to the original index
    /// 3: Insert entries into the local BTreeMap sequentially after it's all over
    pub fn add_files_from_directories(
        &mut self,
        search_dirs: impl IntoParallelIterator<Item = impl AsRef<Path> + Sync>,
    ) {
        self.file_map
            .par_extend(search_dirs.into_par_iter().flat_map(|dir| {
                let dir = dir.as_ref().to_path_buf();

                WalkDir::new(&dir)
                    .into_iter()
                    .filter_map(move |result| match result {
                        Ok(res) => Some((res, dir.clone())),
                        Err(_) => None,
                    })
                    .filter(|(entry, _)| entry.file_type().is_file())
                    .par_bridge()
                    .map(move |(entry, base_path)| {
                        let path = entry.path().to_path_buf();

                        let relative_path = path.strip_prefix(&base_path).unwrap_or(&path);

                        let normalized_path = Self::normalize_path(&relative_path);
                        let vfs_file = VfsFile::new(path);
                        (normalized_path, Arc::new(vfs_file))
                    })
            }))
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

impl Index<&str> for VFS {
    type Output = VfsFile;

    fn index(&self, index: &str) -> &Self::Output {
        let normalized_path = Self::normalize_path(index);

        // If the path exists in the file_map, return the file, otherwise return a default value
        self.file_map
            .get(&normalized_path)
            .map(|file| file.as_ref()) // Dereference Arc<VfsFile> to &VfsFile
            .unwrap_or_else(|| {
                static DEFAULT_FILE: std::sync::OnceLock<VfsFile> = std::sync::OnceLock::new();
                DEFAULT_FILE.get_or_init(|| VfsFile::default())
            })
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
            path.display(),
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
            path.display(),
            contents.len()
        );
    }
}
