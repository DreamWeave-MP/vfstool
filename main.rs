use rayon::prelude::*;
use walkdir::{Error as WalkError, WalkDir};

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fs::File as StdFile,
    io::{self, Read, Seek},
    ops::Index,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

// Owned
type VFSFiles = HashMap<PathBuf, Arc<VfsFile>>;

// With lifetimes
type DisplayTree<'a> = BTreeMap<Cow<'a, str>, Vec<Cow<'a, str>>>;
type MaybeFile<'a> = Option<&'a Arc<VfsFile>>;
type VFSTuple<'a> = (&'a Path, &'a Arc<VfsFile>);

// Define a new trait that combines Read and Seek
trait ReadSeek: Read + Seek {}

// Explicitly implement the ReadSeek trait for std::fs::File
impl ReadSeek for StdFile {}

// This trait mimics the interface of OpenMW's `File`
trait File {
    fn open(&self) -> io::Result<Box<dyn ReadSeek>>;
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

impl PartialEq<VfsFile> for &VfsFile {
    fn eq(&self, other: &VfsFile) -> bool {
        self == other
    }
}

struct VFS {
    file_map: VFSFiles,
}

impl VFS {
    const DIR_PREFIX: &str = "├── ";
    const FILE_PREFIX: &str = "│   ├── ";

    pub fn new() -> Self {
        Self {
            file_map: HashMap::new(),
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
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> MaybeFile {
        let normalized_path = Self::normalize_path(path.as_ref());
        self.file_map.get(&normalized_path)
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(&self, substring: S) -> impl Iterator<Item = VFSTuple> {
        let normalized_substring = Self::normalize_path(substring.as_ref())
            .to_string_lossy()
            .into_owned();

        self.file_map.iter().filter_map(move |(path, file)| {
            if path.to_string_lossy().contains(&normalized_substring) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn par_paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl ParallelIterator<Item = VFSTuple> {
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
    pub fn paths_with<P: AsRef<Path>>(&self, prefix: P) -> impl Iterator<Item = VFSTuple> {
        let normalized_prefix = Self::normalize_path(&prefix);

        self.file_map.iter().filter_map(move |(path, file)| {
            if path.starts_with(&normalized_prefix) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn par_paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl ParallelIterator<Item = VFSTuple> {
        let normalized_prefix = Self::normalize_path(&prefix);

        self.file_map.par_iter().filter_map(move |(path, file)| {
            if path.starts_with(&normalized_prefix) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Walkdir helper to filter out directories
    /// and somehow-nonexistent or inaccessible files
    fn valid_file(entry: Result<walkdir::DirEntry, WalkError>) -> Option<walkdir::DirEntry> {
        match entry {
            Err(_) => None,
            Ok(entry) => match entry.metadata().is_ok() && entry.file_type().is_file() {
                true => Some(entry),
                false => None,
            },
        }
    }

    /// Given some set which can be interpreted as a parallel iterator of paths,
    /// Load all of them into the VFS in parallel fashion
    pub fn add_files_from_directories(
        &mut self,
        search_dirs: impl IntoParallelIterator<Item = impl AsRef<Path> + Sync>,
    ) {
        self.file_map
            .par_extend(search_dirs.into_par_iter().flat_map(|dir| {
                let dir = dir.as_ref().to_path_buf();

                WalkDir::new(&dir)
                    .into_iter()
                    .filter_map(|entry| Self::valid_file(entry))
                    .par_bridge()
                    .map(move |entry| {
                        let path = entry.path().to_path_buf();

                        let normalized_path =
                            Self::normalize_path(&path.strip_prefix(&dir).unwrap_or(&path));

                        let vfs_file = VfsFile::new(path);
                        (normalized_path, Arc::new(vfs_file))
                    })
            }))
    }

    /// Returns a sorted version of the VFS contents as a binary tree
    /// Easier to display.
    pub fn tree(&self, relative: bool) -> DisplayTree {
        let mut tree: DisplayTree = BTreeMap::new();

        let mut paths: Vec<_> = match relative {
            true => self.file_map.keys().collect(),
            false => self.file_map.values().map(|entry| &entry.path).collect(),
        };

        paths.sort();

        paths.iter().for_each(|path| {
            let mut components = path.components();

            if let Some(Component::Normal(file)) = components.next_back() {
                let dir = components.as_path();

                let dir_str = if dir.as_os_str().is_empty() {
                    Cow::Borrowed("/")
                } else {
                    Cow::Owned(dir.to_string_lossy().into_owned())
                };

                let entry_str = Cow::Owned(file.to_string_lossy().into_owned());

                tree.entry(dir_str).or_default().push(entry_str);
            }
        });

        tree
    }

    /// Return a matching set of vfs entries from filter predicates for directories and files
    /// Might be empty.
    pub fn tree_filtered(
        &self,
        relative: bool,
        dir_filter: impl Fn(&str) -> bool,
        file_filter: impl Fn(&str) -> bool,
    ) -> DisplayTree {
        let mut tree = self.tree(relative);

        tree.retain(|dir, files| {
            files.retain(|file| file_filter(file));
            dir_filter(dir) && !files.is_empty()
        });

        tree
    }

    /// String formatter for the file tree
    /// Includes a newline, so caller is responsible for using the appropriate writer
    fn file_str<S: AsRef<str> + std::fmt::Display>(file: S) -> String {
        format!("{}{}\n", Self::FILE_PREFIX, file,)
    }

    fn dir_str<S: AsRef<str> + std::fmt::Display>(dir: S) -> String {
        format!("{}{}/\n", Self::DIR_PREFIX, dir,)
    }

    /// Returns the formatted file tree for a filtered subset
    pub fn display_filtered<'a>(
        &self,
        relative: bool,
        dir_filter: impl Fn(&str) -> bool,
        file_filter: impl Fn(&str) -> bool,
    ) -> String {
        let tree = self.tree(relative);
        let mut output = String::new();

        for (dir, mut files) in tree {
            if !dir_filter(&dir) {
                continue;
            }

            files.retain(|file| file_filter(file));

            if files.is_empty() {
                continue;
            } else if dir != "/" {
                output.push_str(&Self::dir_str(&dir));
            }

            files
                .iter()
                .for_each(|file| output.push_str(&Self::file_str(file)));
        }

        output
    }
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "/")?;
        for (dir, files) in &self.tree(true) {
            if dir != "/" {
                write!(f, "{}", Self::dir_str(dir))?;
            }
            for file in files {
                write!(f, "{}", Self::file_str(file))?;
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
