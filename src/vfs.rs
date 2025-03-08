use rayon::prelude::*;
use serde::{Serialize, Serializer, ser::SerializeMap};
use walkdir::{Error as WalkError, WalkDir};

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fmt,
    fs::File as StdFile,
    io::{self, Read, Seek, Write},
    ops::Index,
    path::{Path, PathBuf},
    sync::Arc,
};

// Owned
type VFSFiles = HashMap<PathBuf, Arc<VfsFile>>;
pub type DisplayTree = BTreeMap<PathBuf, DirectoryNode>;

pub enum SerializeType {
    Json,
    Yaml,
    Toml,
}

pub trait VFSSerialize {
    fn to_serialized<P: AsRef<Path>>(
        &self,
        file_name: P,
        write_type: SerializeType,
    ) -> io::Result<()>;
}

impl VFSSerialize for DisplayTree {
    fn to_serialized<P: AsRef<Path>>(
        &self,
        file_name: P,
        write_type: SerializeType,
    ) -> io::Result<()> {
        fn to_io_error<E: std::fmt::Display>(err: E) -> io::Error {
            io::Error::new(io::ErrorKind::InvalidData, err.to_string())
        }

        let serialized_content = match write_type {
            SerializeType::Json => serde_json::to_string_pretty(&self).map_err(to_io_error)?,
            SerializeType::Yaml => serde_yaml_with_quirks::to_string(&self).map_err(to_io_error)?,
            SerializeType::Toml => toml::to_string_pretty(&self).map_err(to_io_error)?,
        };

        let mut output_file = StdFile::create(file_name)?;
        write!(output_file, "{}", serialized_content)?;

        Ok(())
    }
}

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

trait VFSDirectory {
    fn sort(&mut self);

    fn filter<F>(&mut self, file_filter: &F)
    where
        F: Fn(&Arc<VfsFile>) -> bool;
}

impl VFSDirectory for DirectoryNode {
    fn sort(&mut self) {
        self.files
            .sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
        self.subdirs.values_mut().for_each(|dir| dir.sort());
    }

    fn filter<F>(&mut self, file_filter: &F)
    where
        F: Fn(&Arc<VfsFile>) -> bool,
    {
        self.files.retain(file_filter);
        self.subdirs.retain(|_path, subdir| {
            subdir.filter(file_filter);
            !subdir.files.is_empty() || !subdir.subdirs.is_empty()
        });
    }
}

/// Struct representing a file in the VFS
#[derive(Debug)]
pub struct VfsFile {
    pub path: PathBuf,
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

#[derive(Debug)]
pub struct DirectoryNode {
    files: Vec<Arc<VfsFile>>,
    subdirs: DisplayTree,
}

impl DirectoryNode {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            subdirs: BTreeMap::new(),
        }
    }
}

impl Serialize for DirectoryNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(
            self.subdirs.len() + if self.files.is_empty() { 0 } else { 1 },
        ))?;

        if !self.files.is_empty() {
            map.serialize_entry(
                ".",
                &self
                    .files
                    .iter()
                    .map(|file| file.path.file_name().unwrap_or_default().to_string_lossy())
                    .collect::<Vec<Cow<'_, str>>>(),
            )?;
        }

        for (dir_name, subdir) in &self.subdirs {
            let dir_key = dir_name.file_name().unwrap_or_default().to_string_lossy();

            map.serialize_entry(&dir_key, subdir)?;
        }

        map.end()
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

pub struct VFS {
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
        let root_path = PathBuf::from(match relative {
            true => "Data Files",
            false => "/",
        });

        tree.insert(root_path.clone(), DirectoryNode::new());

        for (key, entry) in &self.file_map {
            let path = if relative { key } else { &entry.path };
            let parent = path.parent().unwrap_or(&root_path);

            let mut current_path = PathBuf::new();
            let mut current_node = tree
                .get_mut(&root_path.clone())
                .expect("Root path should be guaranteed to always exist!");

            for component in parent.components() {
                current_path.push(component);

                if current_path == root_path {
                    continue;
                }

                current_node = current_node
                    .subdirs
                    .entry(current_path.clone())
                    .or_insert_with(DirectoryNode::new);
            }

            current_node.files.push(entry.clone());
        }

        tree.get_mut(&root_path)
            .expect("Root path should be guaranteed to always exist!")
            .sort();

        tree
    }

    /// Return a matching set of vfs entries from filter predicates for directories and files
    /// Might be empty.
    pub fn tree_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&Arc<VfsFile>) -> bool,
    ) -> DisplayTree {
        let mut tree = self.tree(relative);

        tree.iter_mut().for_each(|(_root_dir, files)| {
            dbg!(&_root_dir);
            files.filter(&file_filter);
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
        file_filter: impl Fn(&Arc<VfsFile>) -> bool,
    ) -> String {
        use fmt::Write;

        let tree = self.tree_filtered(relative, file_filter);
        let mut output = String::new();

        for (dir, files) in &tree {
            write!(output, "{}", Self::dir_str(&dir.to_string_lossy())).unwrap();
            for file in &files.files {
                write!(
                    output,
                    "{}",
                    Self::file_str(&file.path.file_name().unwrap().to_string_lossy())
                )
                .unwrap();
            }
        }

        output
    }
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (dir, files) in &self.tree(true) {
            let os_dir = dir.to_string_lossy();
            write!(f, "{}", Self::dir_str(os_dir))?;
            for file in &files.files {
                write!(
                    f,
                    "{}",
                    Self::file_str(file.path.file_name().unwrap().to_string_lossy())
                )?;
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
