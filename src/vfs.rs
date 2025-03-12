use rayon::prelude::*;
use walkdir::WalkDir;

use crate::{DirectoryNode, DisplayTree, SerializeType, VfsFile, normalize_path};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    fs::File,
    io::{Error, ErrorKind, Result, Write as _},
    ops::Index,
    path::{Path, PathBuf},
};

// Owned
type MaybeFile<'a> = Option<&'a VfsFile>;
type VFSTuple<'a> = (&'a Path, &'a VfsFile);
type VFSFiles = HashMap<PathBuf, VfsFile>;

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

    /// Returns a parallel iterator meant to be fed into par_extend
    /// Only used when appending a directory or set of directories into the file map
    fn directory_contents_to_file_map<I: AsRef<Path> + Sync>(
        dir: I,
    ) -> impl ParallelIterator<Item = (PathBuf, VfsFile)> {
        let dir = dir.as_ref().to_path_buf();

        WalkDir::new(&dir)
            .into_iter()
            .filter_map(|entry| entry.ok().filter(|e| e.file_type().is_file()))
            .par_bridge()
            .map(move |entry| {
                let path = entry.path();
                let target_path = &path.strip_prefix(&dir).unwrap_or(&path);

                let normalized_path = normalize_path(target_path);

                let vfs_file = VfsFile::from(path);
                (normalized_path, vfs_file)
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
        archive_list: Vec<&str>,
    ) -> Self {
        let mut vfs = Self::new();

        let map: HashMap<PathBuf, VfsFile> = search_dirs
            .into_par_iter()
            .flat_map(Self::directory_contents_to_file_map)
            .collect();

        let archive_handles = crate::archives::from_set(&map, archive_list);

        vfs.file_map
            .par_extend(crate::archives::file_map(&archive_handles));

        vfs.file_map.par_extend(map);

        vfs
    }

    /// Returns a sorted version of the VFS contents as a binary tree
    /// Easier to display.
    pub fn tree(&self, relative: bool) -> DisplayTree {
        let mut tree: DisplayTree = BTreeMap::new();
        let root_path: PathBuf = if relative { "Data Files" } else { "/" }.into();

        tree.insert(root_path.clone(), DirectoryNode::new());

        for (key, entry) in &self.file_map {
            let path = PathBuf::from(
                if relative {
                    entry.parent_archive_name()
                } else {
                    entry.parent_archive_path()
                }
                .map_or_else(
                    || {
                        if relative {
                            key.into()
                        } else {
                            entry.path().to_path_buf()
                        }
                    },
                    |parent| PathBuf::from(parent).join(key),
                ),
            );

            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| root_path.as_path());

            let mut current_path = PathBuf::new();
            let mut current_node = tree
                .get_mut(&root_path)
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

            let new_file = match entry.is_archive() {
                false => VfsFile::from(path),
                true => VfsFile::from_archive(
                    path.to_string_lossy(),
                    entry.parent_archive_handle().unwrap(),
                ),
            };

            current_node.files.push(new_file);
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
        file_filter: impl Fn(&VfsFile) -> bool,
    ) -> DisplayTree {
        let mut tree = self.tree(relative);

        tree.iter_mut().for_each(|(_root_dir, files)| {
            files.filter(&file_filter);
        });

        tree
    }

    /// String formatter for the file tree
    /// Includes a newline, so caller is responsible for using the appropriate writer
    fn file_str<S: AsRef<str> + std::fmt::Display>(file: S) -> String {
        format!("{}{}\n", Self::FILE_PREFIX, file,)
    }

    /// String formatter for the file tree
    /// Includes a newline, so caller is responsible for using the appropriate writer
    fn dir_str<S: AsRef<str> + std::fmt::Display>(dir: S) -> String {
        format!("{}{}/\n", Self::DIR_PREFIX, dir,)
    }

    /// Returns the formatted file tree for a filtered subset
    pub fn display_filtered<'a>(
        &self,
        relative: bool,
        file_filter: impl Fn(&VfsFile) -> bool,
    ) -> String {
        let tree = self.tree_filtered(relative, file_filter);
        let mut output = String::new();

        if let Err(error) = write_tree_io(&tree, &mut output) {
            panic!("Failed to format DisplayTree: {}", error)
        };

        output
    }

    /// Serializes the result of `tree` or `display_filtered` functions to JSON, YAML, or TOML
    pub fn serialize_from_tree<P: AsRef<Path>>(
        tree: &DisplayTree,
        file_name: P,
        write_type: SerializeType,
    ) -> Result<()> {
        fn to_io_error<E: std::fmt::Display>(err: E) -> Error {
            Error::new(ErrorKind::InvalidData, err.to_string())
        }

        let serialized_content = match write_type {
            SerializeType::Json => serde_json::to_string_pretty(&tree).map_err(to_io_error)?,
            SerializeType::Yaml => serde_yaml_with_quirks::to_string(&tree).map_err(to_io_error)?,
            SerializeType::Toml => toml::to_string_pretty(&tree).map_err(to_io_error)?,
        };

        let mut output_file = File::create(file_name)?;
        write!(output_file, "{}", serialized_content)?;

        Ok(())
    }
}

fn to_eof_err<E: std::fmt::Display>(error: E) -> std::io::Error {
    Error::new(ErrorKind::UnexpectedEof, error.to_string())
}

fn write_files_io<W: Write>(w: &mut W, node: &DirectoryNode, dir: &PathBuf) -> std::io::Result<()> {
    if !node.files.is_empty() {
        write!(w, "{}", VFS::dir_str(dir.to_string_lossy())).map_err(to_eof_err)?;

        for file in &node.files {
            write!(
                w,
                "{}",
                VFS::file_str(file.path().file_name().unwrap().to_string_lossy())
            )
            .map_err(to_eof_err)?;
        }
    };

    Ok(())
}

fn print_files_fmt(
    f: &mut std::fmt::Formatter<'_>,
    node: &DirectoryNode,
    dir: &PathBuf,
) -> std::fmt::Result {
    if !node.files.is_empty() {
        write!(f, "{}", VFS::dir_str(dir.to_string_lossy()))?;

        for file in &node.files {
            write!(
                f,
                "{}",
                VFS::file_str(file.path().file_name().unwrap().to_string_lossy())
            )?;
        }
    };

    Ok(())
}

fn write_node_io<W: Write>(
    w: &mut W,
    node: &DirectoryNode,
    parent_dir: &PathBuf,
) -> std::io::Result<()> {
    write_files_io(w, &node, parent_dir)?;

    for (subdir_name, subdir_node) in &node.subdirs {
        write_node_io(w, subdir_node, &subdir_name)?;
    }

    Ok(())
}

fn print_node_fmt(
    f: &mut std::fmt::Formatter<'_>,
    node: &DirectoryNode,
    parent_dir: &PathBuf,
) -> std::fmt::Result {
    print_files_fmt(f, &node, parent_dir)?;

    for (subdir_name, subdir_node) in &node.subdirs {
        print_node_fmt(f, subdir_node, &subdir_name)?;
    }

    Ok(())
}

fn write_tree_io<W: Write>(tree: &DisplayTree, f: &mut W) -> std::io::Result<()> {
    for (root_subdir, files) in tree {
        write_files_io(f, files, root_subdir)?;

        for (subdir_name, sub_node) in &files.subdirs {
            write_node_io(f, &sub_node, &subdir_name)?;
        }
    }
    Ok(())
}

fn print_tree_fmt(tree: &DisplayTree, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for (root_subdir, files) in tree {
        print_files_fmt(f, files, root_subdir)?;

        for (subdir_name, sub_node) in &files.subdirs {
            print_node_fmt(f, &sub_node, &subdir_name)?;
        }
    }
    Ok(())
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        print_tree_fmt(&self.tree(true), f)
    }
}

impl Index<&str> for VFS {
    type Output = VfsFile;

    fn index(&self, index: &str) -> &Self::Output {
        let normalized_path = normalize_path(index);

        // If the path exists in the file_map, return the file, otherwise return a default value
        self.file_map.get(&normalized_path).unwrap_or_else(|| {
            static DEFAULT_FILE: std::sync::OnceLock<VfsFile> = std::sync::OnceLock::new();
            DEFAULT_FILE.get_or_init(|| VfsFile::default())
        })
    }
}
