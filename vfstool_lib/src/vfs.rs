use rayon::prelude::*;
use walkdir::WalkDir;

#[cfg(feature = "serialize")]
use crate::SerializeType;
#[cfg(feature = "serialize")]
use std::io::Result;

#[cfg(feature = "bsa")]
use crate::archives;

use crate::{DirectoryNode, DisplayTree, VfsFile, normalize_path};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    io::{Error, ErrorKind},
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
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> MaybeFile<'_> {
        let normalized_path = normalize_path(path);
        self.file_map.get(&normalized_path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &VfsFile)> {
        self.file_map.iter()
    }

    pub fn par_iter(&self) -> impl ParallelIterator<Item = (&PathBuf, &VfsFile)> {
        self.file_map.par_iter()
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl Iterator<Item = VFSTuple<'_>> {
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
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
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
    pub fn paths_with<P: AsRef<Path>>(&self, prefix: P) -> impl Iterator<Item = VFSTuple<'_>> {
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
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
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

    #[allow(unused_variables)]
    pub fn from_directories(
        search_dirs: impl IntoParallelIterator<Item = impl AsRef<Path> + Sync>,
        archive_list: Option<Vec<&str>>,
    ) -> Self {
        let mut vfs = Self::new();

        let map: HashMap<PathBuf, VfsFile> = search_dirs
            .into_par_iter()
            .flat_map(Self::directory_contents_to_file_map)
            .collect();

        #[cfg(feature = "bsa")]
        if let Some(list) = archive_list {
            let archive_handles = archives::from_set(&map, list);

            vfs.file_map.par_extend(archives::file_map(archive_handles));
        }

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
                false => VfsFile::from(entry.path()),
                #[cfg(feature = "bsa")]
                true => VfsFile::from_archive(
                    path.to_string_lossy(),
                    entry.parent_archive_handle().unwrap(),
                ),
                #[cfg(not(feature = "bsa"))]
                true => unimplemented!(
                    "BSA archives are not supported in this build. Enable the 'bsa' feature of vfstool_lib to use them."
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

    /// Return whether any relative path in the vfs corresponds to the absolute path given
    /// Note that the path is normalized by this function, so it's not necessary to do so
    /// beforehand
    pub fn has_normalized_file(&self, target: &Path) -> bool {
        let normalized = normalize_path(target);
        self.file_map
            .keys()
            .any(|relative_path| normalized.ends_with(&relative_path))
    }

    /// Return whether or not a file exists in the VFS at the given absolute path
    /// Note that the path is normalized by this function, so it's not necessary to do so
    /// beforehand
    pub fn has_file(&self, target: &Path) -> bool {
        let normalized = normalize_path(target);
        self.file_map
            .values()
            .any(|file| normalize_path(file.path()).eq(&normalized))
    }

    /// Takes a filesystem path as input and checks if it exists in this VFS
    /// WARNING: This is a real, non-normalized path from the filesystem
    /// Preferably returned by `VfsFile.path()`
    pub fn has_normalized_not_exact(&self, target: &Path) -> bool {
        let normalized = normalize_path(target);
        self.file_map.iter().any(|(relative_path, vfs_file)| {
            vfs_file.path().ne(target) && normalized.ends_with(&relative_path)
        })
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
    #[cfg(feature = "serialize")]
    pub fn serialize_from_tree(tree: &DisplayTree, write_type: SerializeType) -> Result<String> {
        fn to_io_error<E: std::fmt::Display>(err: E) -> Error {
            Error::new(ErrorKind::InvalidData, err.to_string())
        }

        let serialized_content = match write_type {
            SerializeType::Json => serde_json::to_string_pretty(&tree).map_err(to_io_error)?,
            SerializeType::Yaml => serde_yaml_with_quirks::to_string(&tree).map_err(to_io_error)?,
            SerializeType::Toml => toml::to_string_pretty(&tree).map_err(to_io_error)?,
        };

        Ok(serialized_content)
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

#[cfg(test)]
mod tests {
    use super::*;
    use ba2::tes3::{Archive, ArchiveKey, File};
    use std::fs;
    use std::path::PathBuf;

    const TEST_DATA: &[&str] = &[
        "file1.txt",
        "file2.txt",
        "file3.txt",
        "file4.txt",
        "file5.txt",
        "file6.txt",
    ];

    const TEST_STRING: &str = "Act IV, Scene III, continued

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

    fn create_files(dir: &PathBuf, files: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        for file in files {
            let file_path = dir.join(file);
            fs::write(file_path, TEST_STRING).unwrap();
        }
    }

    #[test]
    fn test_vfs_from_directories() {
        let temp_path = std::env::current_dir().unwrap();
        let archive_dir = temp_path.join("archives");

        fs::create_dir_all(&archive_dir).unwrap();

        // Create directories and files
        let (dir1, dir2, dir3) = create_test_dirs_and_files(&temp_path);

        // Create BSA archives
        let bsa1 = create_bsa_archive(&archive_dir, "archive1.bsa", &TEST_DATA[0..6]);
        let bsa2 = create_bsa_archive(&archive_dir, "archive2.bsa", &TEST_DATA[0..5]);
        let bsa3 = create_bsa_archive(&archive_dir, "archive3.bsa", &TEST_DATA[0..4]);

        // Construct VFS
        let search_dirs = vec![
            archive_dir.clone(),
            dir1.clone(),
            dir2.clone(),
            dir3.clone(),
        ];
        let archive_list = vec!["archive1.bsa", "archive2.bsa", "archive3.bsa"];

        let vfs = VFS::from_directories(search_dirs.clone(), Some(archive_list));

        // Verify file locations
        verify_file_locations(&vfs, &bsa1, &bsa2, &bsa3, &dir1, &dir2, &dir3);

        // Clean up test files and directories
        clean_up_test_files(&search_dirs);
    }

    fn create_test_dirs_and_files(temp_path: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let dir1 = temp_path.join("dir1");
        let dir2 = temp_path.join("dir2");
        let dir3 = temp_path.join("dir3");

        create_files(&dir1, &TEST_DATA[0..3]); // file1.txt, file2.txt, file3.txt
        create_files(&dir2, &TEST_DATA[0..2]); // file1.txt, file2.txt
        create_files(&dir3, &TEST_DATA[0..1]); // file1.txt
        create_files(&temp_path.to_path_buf(), &TEST_DATA[..]);

        (dir1, dir2, dir3)
    }

    fn create_bsa_archive(archive_dir: &Path, archive_name: &str, data: &[&str]) -> PathBuf {
        let archive_path = archive_dir.join(archive_name);
        let archive: Archive = data
            .iter()
            .map(|s| {
                let key: ArchiveKey = s.to_string().into();
                let file: File = File::from(s.as_bytes());
                (key, file)
            })
            .collect();
        let mut dst = fs::File::create(&archive_path).unwrap();
        archive.write(&mut dst).unwrap();
        archive_path
    }

    fn verify_file_locations(
        vfs: &VFS,
        bsa1: &PathBuf,
        bsa2: &PathBuf,
        bsa3: &PathBuf,
        dir1: &PathBuf,
        dir2: &PathBuf,
        dir3: &PathBuf,
    ) {
        assert_eq!(
            vfs.file_map
                .get(&PathBuf::from("file6.txt"))
                .unwrap()
                .parent_archive_path()
                .unwrap(),
            bsa1.to_str().unwrap()
        );

        assert_eq!(
            vfs.file_map
                .get(&PathBuf::from("file5.txt"))
                .unwrap()
                .parent_archive_path()
                .unwrap(),
            bsa2.to_str().unwrap()
        );

        assert_eq!(
            vfs.file_map
                .get(&PathBuf::from("file4.txt"))
                .unwrap()
                .parent_archive_path()
                .unwrap(),
            bsa3.to_str().unwrap()
        );

        assert_eq!(
            vfs.file_map
                .get(&PathBuf::from("file3.txt"))
                .unwrap()
                .path(),
            dir1.join("file3.txt")
        );

        assert_eq!(
            vfs.file_map
                .get(&PathBuf::from("file2.txt"))
                .unwrap()
                .path(),
            dir2.join("file2.txt")
        );

        assert_eq!(
            vfs.file_map
                .get(&PathBuf::from("file1.txt"))
                .unwrap()
                .path(),
            dir3.join("file1.txt")
        );
    }

    fn clean_up_test_files(search_dirs: &[PathBuf]) {
        search_dirs
            .iter()
            .for_each(|dir| fs::remove_dir_all(dir).unwrap());
        TEST_DATA
            .iter()
            .for_each(|test_file| fs::remove_file(test_file).unwrap());
    }
}
