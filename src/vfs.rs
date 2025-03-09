use rayon::prelude::*;
use walkdir::{Error as WalkError, WalkDir};

use crate::{DirectoryNode, DisplayTree, VFSDirectory, VfsFile, normalize_path};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    ops::Index,
    path::{Path, PathBuf},
    sync::Arc,
};

// Owned
type MaybeFile<'a> = Option<&'a Arc<VfsFile>>;
type VFSTuple<'a> = (&'a Path, &'a Arc<VfsFile>);
type VFSFiles = HashMap<PathBuf, Arc<VfsFile>>;

pub struct VFS {
    file_map: VFSFiles,
}

impl VFS {
    const DIR_PREFIX: &str = "├── ";
    const FILE_PREFIX: &str = "│   ├── ";

    fn new() -> Self {
        Self {
            file_map: HashMap::new(),
        }
    }

    /// Looks up a file in the VFS after normalizing the path
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> MaybeFile {
        let normalized_path = normalize_path(path);
        self.file_map.get(&normalized_path)
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(&self, substring: S) -> impl Iterator<Item = VFSTuple> {
        let normalized_substring = normalize_path(substring.as_ref())
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
        let normalized_substring = normalize_path(substring.as_ref())
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
        let normalized_prefix = normalize_path(&prefix);

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
        let normalized_prefix = normalize_path(&prefix);

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

    /// Returns a parallel iterator meant to be fed into par_extend
    /// Only used when appending a directory or set of directories into the file map
    fn directory_contents_to_file_map<I: AsRef<Path> + Sync>(
        dir: I,
    ) -> impl ParallelIterator<Item = (PathBuf, Arc<VfsFile>)> {
        let dir = dir.as_ref().to_path_buf();

        WalkDir::new(&dir)
            .into_iter()
            .filter_map(|entry| Self::valid_file(entry))
            .par_bridge()
            .map(move |entry| {
                let path = entry.path().to_path_buf();

                let normalized_path = normalize_path(&path.strip_prefix(&dir).unwrap_or(&path));

                let vfs_file = VfsFile::new(path);
                (normalized_path, Arc::new(vfs_file))
            })
    }

    /// Append a single directory to the existing VFS instance
    /// NOTE: Writing directories in sequence can be dangerous and should be avoided when possible!
    /// When a directory (or set) is appended after the initial creation time, this may be useful,
    /// but it will also overwrite the contents of *all* directories added before it
    /// Use this functionality *with caution*
    pub fn add_directory<I: AsRef<Path> + Sync>(mut self, dir: I) -> Self {
        self.file_map
            .par_extend(Self::directory_contents_to_file_map(dir));
        self
    }

    /// Given some set which can be interpreted as a parallel iterator of paths,
    /// Load all of them into the VFS in parallel fashion
    /// WARN: When a directory (or set) is appended after the initial creation time, this may be useful,
    /// but it will also overwrite the contents of *all* directories added before it
    /// Use this functionality *with caution*
    pub fn add_directories(
        mut self,
        search_dirs: impl IntoParallelIterator<Item = impl AsRef<Path> + Sync>,
    ) -> Self {
        self.file_map.par_extend(
            search_dirs
                .into_par_iter()
                .flat_map(Self::directory_contents_to_file_map),
        );
        self
    }

    pub fn from_directory<I: AsRef<Path> + Sync>(dir: I) -> Self {
        Self::new().add_directory(dir)
    }

    pub fn from_directories(
        search_dirs: impl IntoParallelIterator<Item = impl AsRef<Path> + Sync>,
    ) -> Self {
        Self::new().add_directories(search_dirs)
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
            let path = if relative { key } else { entry.path() };
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
                    Self::file_str(&file.path().file_name().unwrap().to_string_lossy())
                )
                .unwrap();
            }
        }

        output
    }
}

/// Currently only prints the root directory
/// Needs to iterate over all child directories and print those as well.
impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (dir, files) in &self.tree(true) {
            let os_dir = dir.to_string_lossy();
            write!(f, "{}", Self::dir_str(os_dir))?;
            for file in &files.files {
                write!(
                    f,
                    "{}",
                    Self::file_str(file.path().file_name().unwrap().to_string_lossy())
                )?;
            }
        }
        Ok(())
    }
}

impl Index<&str> for VFS {
    type Output = VfsFile;

    fn index(&self, index: &str) -> &Self::Output {
        let normalized_path = normalize_path(index);

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
