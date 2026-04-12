use rayon::prelude::*;
use walkdir::WalkDir;

#[cfg(feature = "serialize")]
use crate::SerializeType;
#[cfg(feature = "serialize")]
use std::io::{Error, ErrorKind, Result};

#[cfg(feature = "bsa")]
use crate::archives;

use crate::{DirectoryNode, DisplayTree, VfsFile, normalize_path, normalize_path_in_place};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
    path::{Path, PathBuf},
};

// Owned
type MaybeFile<'a> = Option<&'a VfsFile>;
type VFSTuple<'a> = (&'a Path, &'a VfsFile);
type VFSFiles = HashMap<PathBuf, VfsFile>;

/// Result of scanning a directory against a [`VFS`].
///
/// Produced by [`VFS::diff_directory`]. Each scanned file falls into exactly
/// one of two categories:
///
/// - **`conflicts`** — the file exists in both the directory and the VFS.
///   Each entry pairs the incoming file (from the directory) with the file
///   currently in the VFS that it would displace.
///
/// - **`additions`** — the file exists only in the directory; it would be a
///   net-new entry in the VFS.
pub struct DirectoryDiff<'vfs> {
    /// Files present in both the directory and the VFS.
    /// `(normalized_key, incoming_file, current_vfs_entry)`.
    pub conflicts: Vec<(PathBuf, VfsFile, &'vfs VfsFile)>,

    /// Files in the directory that are not in the VFS.
    pub additions: Vec<(PathBuf, VfsFile)>,
}

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
        let needle = Self::normalize_substring(substring);
        self.file_map.iter().filter_map(move |(path, file)| {
            path.to_string_lossy().contains(&needle).then_some((path.as_path(), file))
        })
    }

    /// Given a substring, return a parallel iterator over all paths that contain it.
    pub fn par_paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let needle = Self::normalize_substring(substring);
        self.file_map.par_iter().filter_map(move |(path, file)| {
            path.to_string_lossy().contains(&needle).then_some((path.as_path(), file))
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn paths_with<P: AsRef<Path>>(&self, prefix: P) -> impl Iterator<Item = VFSTuple<'_>> {
        let normalized_prefix = normalize_path(&prefix);
        self.file_map.iter().filter_map(move |(path, file)| {
            path.starts_with(&normalized_prefix).then_some((path.as_path(), file))
        })
    }

    /// Given a path prefix to a location in the VFS, return a parallel iterator to *all* of its contents.
    pub fn par_paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let normalized_prefix = normalize_path(&prefix);
        self.file_map.par_iter().filter_map(move |(path, file)| {
            path.starts_with(&normalized_prefix).then_some((path.as_path(), file))
        })
    }

    fn normalize_substring<S: AsRef<str>>(s: S) -> String {
        normalize_path(s.as_ref()).to_string_lossy().into_owned()
    }

    /// Returns a parallel iterator meant to be fed into par_extend
    /// Only used when appending a directory or set of directories into the file map
    fn directory_contents_to_file_map<I: AsRef<Path> + Sync>(
        dir: I,
    ) -> impl ParallelIterator<Item = (PathBuf, VfsFile)> {
        let dir = dir.as_ref().to_path_buf();

        WalkDir::new(&dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|entry| entry.ok().filter(|e| e.file_type().is_file()))
            .par_bridge()
            .map(move |entry| {
                let path = entry.path();
                let target_path = path
                    .strip_prefix(&dir)
                    .expect("Entry path should always be prefixed by scan directory!");

                let mut normalized_path = target_path.to_path_buf();
                normalize_path_in_place(&mut normalized_path);

                let vfs_file = VfsFile::from(path);
                (normalized_path, vfs_file)
            })
    }

    pub fn from_directories(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(not(feature = "bsa"), allow(unused_variables))]
        archive_list: Option<Vec<&str>>,
    ) -> Self {
        let mut vfs = Self::new();

        let dir_maps: Vec<HashMap<PathBuf, VfsFile>> = search_dirs
            .into_iter()
            .map(|dir| Self::directory_contents_to_file_map(dir).collect())
            .collect();

        #[cfg(feature = "bsa")]
        if let Some(list) = archive_list {
            let loose_lookup: HashMap<PathBuf, VfsFile> = dir_maps
                .iter()
                .flat_map(|m| m.iter().map(|(k, v)| (k.clone(), VfsFile::from(v.path()))))
                .collect();
            let archive_handles = archives::from_set(&loose_lookup, &list);
            vfs.file_map.par_extend(archives::file_map(archive_handles));
        }

        // Merge directories in order: later directories override earlier ones,
        // matching OpenMW's VFS semantics (last data= entry wins).
        for map in dir_maps {
            vfs.file_map.extend(map);
        }

        vfs
    }

    /// Scans `dir` and classifies every file against this VFS.
    ///
    /// Each file found under `dir` falls into one of two categories:
    ///
    /// - **conflict** — a file at the same normalized path already exists in
    ///   the VFS. The returned entry pairs the incoming file with the VFS entry
    ///   it would displace.
    /// - **addition** — no file at that path exists in the VFS; installing this
    ///   directory would add it as a new entry.
    ///
    /// The directory is walked in parallel (rayon). Lookups against the VFS map
    /// are O(1) per file. This is the primitive for mod conflict analysis: call
    /// it once per candidate directory to get the full picture of what that mod
    /// installs, overrides, and adds.
    pub fn diff_directory<'vfs, P: AsRef<Path> + Sync>(
        &'vfs self,
        dir: P,
    ) -> DirectoryDiff<'vfs> {
        let dir = dir.as_ref().to_path_buf();

        // Walk the directory in parallel — I/O is the bottleneck here.
        let entries: Vec<(PathBuf, VfsFile)> = WalkDir::new(&dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok().filter(|e| e.file_type().is_file()))
            .par_bridge()
            .map(|entry| {
                let mut normalized = entry
                    .path()
                    .strip_prefix(&dir)
                    .expect("entry path must be prefixed by scan directory")
                    .to_path_buf();
                normalize_path_in_place(&mut normalized);
                (normalized, VfsFile::from(entry.path()))
            })
            .collect();

        // Single O(1) HashMap lookup per entry — no VFS iteration required.
        let mut conflicts = Vec::new();
        let mut additions = Vec::new();

        for (key, incoming) in entries {
            match self.file_map.get(&key) {
                Some(existing) => conflicts.push((key, incoming, existing)),
                None => additions.push((key, incoming)),
            }
        }

        DirectoryDiff { conflicts, additions }
    }

    /// Returns a sorted version of the VFS contents as a binary tree.
    pub fn tree(&self, relative: bool) -> DisplayTree {
        self.build_tree(relative, None::<fn(&VfsFile) -> bool>)
    }

    /// Returns a sorted tree containing only files accepted by `file_filter`.
    ///
    /// Unlike the old two-pass implementation (build full tree then prune),
    /// this filters during construction: directory nodes are only created for
    /// paths that contain at least one accepted file, so no separate prune pass
    /// is required.
    pub fn tree_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&VfsFile) -> bool,
    ) -> DisplayTree {
        self.build_tree(relative, Some(file_filter))
    }

    fn build_tree<F: Fn(&VfsFile) -> bool>(
        &self,
        relative: bool,
        file_filter: Option<F>,
    ) -> DisplayTree {
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

            // Filter before touching the tree so we never create directory
            // nodes for paths whose files are all excluded.
            if file_filter.as_ref().is_some_and(|f| !f(&new_file)) {
                continue;
            }

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

                let component_name = PathBuf::from(component.as_os_str());
                current_node = current_node
                    .subdirs
                    .entry(component_name)
                    .or_insert_with(DirectoryNode::new);
            }

            current_node.files.push(new_file);
        }

        tree.get_mut(&root_path)
            .expect("Root path should be guaranteed to always exist!")
            .sort();

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
        let norm_bytes = normalized.as_os_str().as_encoded_bytes();
        self.file_map.values().any(|file| {
            let file_bytes = file.path().as_os_str().as_encoded_bytes();
            file_bytes.len() == norm_bytes.len()
                && file_bytes.iter().zip(norm_bytes.iter()).all(|(&fb, &nb)| {
                    let nfb = match fb {
                        b'\\' => b'/',
                        b'A'..=b'Z' => fb + 32,
                        _ => fb,
                    };
                    nfb == nb
                })
        })
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
    pub fn display_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&VfsFile) -> bool,
    ) -> String {
        let tree = self.tree_filtered(relative, file_filter);
        let mut output = String::new();
        write_tree(&tree, &mut output).expect("String fmt::Write cannot fail");
        output
    }

    /// Serializes the result of `tree` or `display_filtered` functions to JSON, YAML, or TOML
    #[cfg(feature = "serialize")]
    pub fn serialize_from_tree(tree: &DisplayTree, write_type: SerializeType) -> Result<String> {
        fn to_io_error<E: std::fmt::Display>(err: E) -> Error {
            Error::new(ErrorKind::InvalidData, err.to_string())
        }

        let serialized_content = match write_type {
            SerializeType::Json => serde_json::to_string(&tree).map_err(to_io_error)?,
            SerializeType::Yaml => serde_yaml::to_string(&tree).map_err(to_io_error)?,
            SerializeType::Toml => toml::to_string_pretty(&tree).map_err(to_io_error)?,
        };

        Ok(serialized_content)
    }
}

fn write_node<W: Write>(w: &mut W, node: &DirectoryNode, dir: &PathBuf) -> std::fmt::Result {
    if !node.files.is_empty() {
        write!(w, "{}", VFS::dir_str(dir.to_string_lossy()))?;
        for file in &node.files {
            write!(
                w,
                "{}",
                VFS::file_str(file.path().file_name().unwrap().to_string_lossy())
            )?;
        }
    }
    for (subdir_name, subdir_node) in &node.subdirs {
        write_node(w, subdir_node, subdir_name)?;
    }
    Ok(())
}

fn write_tree<W: Write>(tree: &DisplayTree, w: &mut W) -> std::fmt::Result {
    for (root_subdir, root_node) in tree {
        write_node(w, root_node, root_subdir)?;
    }
    Ok(())
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_tree(&self.tree(true), f)
    }
}


#[cfg(test)]
mod loose_tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    /// RAII temp directory scoped to a named subdirectory of cwd.
    /// Cleaned up on drop so panics in tests don't leave debris.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::current_dir().unwrap().join(name);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Write `data` to `rel` (relative to this dir), creating intermediate dirs.
        fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
            let target = self.0.join(rel);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, data).unwrap();
            target
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Recursively collect all file names from a DirectoryNode tree.
    fn collect_all_filenames(node: &DirectoryNode) -> Vec<String> {
        let mut names: Vec<String> = node
            .files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        for sub in node.subdirs.values() {
            names.extend(collect_all_filenames(sub));
        }
        names
    }

    // ---- Construction ----

    #[test]
    fn from_empty_directory_yields_empty_vfs() {
        let dir = TempDir::new("vfsloose_empty");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert_eq!(vfs.iter().count(), 0);
    }

    #[test]
    fn from_single_directory_all_files_present() {
        let dir = TempDir::new("vfsloose_single");
        dir.write("foo.txt", b"a");
        dir.write("bar.txt", b"b");
        dir.write("sub/baz.txt", b"c");

        let vfs = VFS::from_directories(vec![dir.path()], None);

        assert!(vfs.get_file("foo.txt").is_some());
        assert!(vfs.get_file("bar.txt").is_some());
        assert!(vfs.get_file("sub/baz.txt").is_some());
        assert_eq!(vfs.iter().count(), 3);
    }

    #[test]
    fn from_multiple_directories_unique_files_all_present() {
        let dir1 = TempDir::new("vfsloose_multi1");
        let dir2 = TempDir::new("vfsloose_multi2");
        dir1.write("only_in_1.txt", b"1");
        dir2.write("only_in_2.txt", b"2");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

        assert!(vfs.get_file("only_in_1.txt").is_some());
        assert!(vfs.get_file("only_in_2.txt").is_some());
    }

    #[test]
    fn from_directories_recurses_deeply() {
        let dir = TempDir::new("vfsloose_deep");
        dir.write("a/b/c/d/deep.txt", b"deep");

        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.get_file("a/b/c/d/deep.txt").is_some());
    }

    // ---- get_file ----

    #[test]
    fn get_file_exact_lowercase_key() {
        let dir = TempDir::new("vfsloose_get_exact");
        dir.write("meshes/foo.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.get_file("meshes/foo.nif").is_some());
    }

    #[test]
    fn get_file_case_insensitive() {
        let dir = TempDir::new("vfsloose_get_case");
        dir.write("meshes/foo.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.get_file("Meshes/Foo.NIF").is_some());
        assert!(vfs.get_file("MESHES/FOO.NIF").is_some());
        assert!(vfs.get_file("mEsHeS/fOo.nIf").is_some());
    }

    #[test]
    fn get_file_backslash_lookup() {
        let dir = TempDir::new("vfsloose_get_backslash");
        dir.write("meshes/foo.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.get_file("meshes\\foo.nif").is_some());
        assert!(vfs.get_file("Meshes\\Foo.NIF").is_some());
    }

    #[test]
    fn get_file_nonexistent_returns_none() {
        let dir = TempDir::new("vfsloose_get_none");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.get_file("does_not_exist.txt").is_none());
    }

    #[test]
    fn get_file_path_confirmed_correct() {
        // get_file must return the actual on-disk path, not the normalized key
        let dir = TempDir::new("vfsloose_get_path");
        let written = dir.write("Meshes/XBase_Anim.NIF", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let file = vfs.get_file("meshes/xbase_anim.nif").unwrap();
        assert_eq!(file.path(), written);
    }

    // ---- priority / collision ----

    /// Core invariant: later directory in the list overrides earlier one.
    /// This mirrors OpenMW's data= ordering semantics.
    #[test]
    fn later_dir_wins_over_earlier_for_same_file() {
        let dir1 = TempDir::new("vfsprio_later_wins_dir1");
        let dir2 = TempDir::new("vfsprio_later_wins_dir2");
        let path1 = dir1.write("shared.txt", b"from_dir1");
        let path2 = dir2.write("shared.txt", b"from_dir2");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

        assert_eq!(vfs.iter().count(), 1, "collision should collapse to one entry");
        let winner = vfs.get_file("shared.txt").unwrap();
        assert_eq!(winner.path(), path2, "dir2 (later) should override dir1 (earlier)");
        assert_ne!(winner.path(), path1);
    }

    #[test]
    fn earlier_dir_does_not_win_over_later_dir() {
        let dir1 = TempDir::new("vfsprio_earlier_loses_dir1");
        let dir2 = TempDir::new("vfsprio_earlier_loses_dir2");
        dir1.write("shared.txt", b"loser");
        let path2 = dir2.write("shared.txt", b"winner");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);
        assert_eq!(vfs.get_file("shared.txt").unwrap().path(), path2);
    }

    #[test]
    fn three_dirs_last_one_wins() {
        let dir1 = TempDir::new("vfsprio_three_dir1");
        let dir2 = TempDir::new("vfsprio_three_dir2");
        let dir3 = TempDir::new("vfsprio_three_dir3");
        dir1.write("shared.txt", b"1");
        dir2.write("shared.txt", b"2");
        let path3 = dir3.write("shared.txt", b"3");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path(), dir3.path()], None);
        assert_eq!(vfs.iter().count(), 1);
        assert_eq!(vfs.get_file("shared.txt").unwrap().path(), path3);
    }

    #[test]
    fn partial_overlap_unique_files_present_and_shared_resolves_to_later() {
        let dir1 = TempDir::new("vfsprio_partial_dir1");
        let dir2 = TempDir::new("vfsprio_partial_dir2");
        let only1 = dir1.write("only_in_1.txt", b"1");
        dir1.write("shared.txt", b"from_dir1");
        let only2 = dir2.write("only_in_2.txt", b"2");
        let shared2 = dir2.write("shared.txt", b"from_dir2");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

        assert_eq!(vfs.iter().count(), 3, "3 unique VFS paths expected");
        assert_eq!(vfs.get_file("only_in_1.txt").unwrap().path(), only1);
        assert_eq!(vfs.get_file("only_in_2.txt").unwrap().path(), only2);
        assert_eq!(vfs.get_file("shared.txt").unwrap().path(), shared2);
    }

    /// Case-folding means paths differing only in case collide on the same VFS
    /// key — the usual priority rules still apply.
    #[test]
    fn collision_across_dirs_via_case_normalization() {
        let dir1 = TempDir::new("vfsprio_case_dir1");
        let dir2 = TempDir::new("vfsprio_case_dir2");
        dir1.write("Textures/Foo.DDS", b"dir1");
        let path2 = dir2.write("textures/foo.dds", b"dir2");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

        assert_eq!(vfs.iter().count(), 1, "case variants are the same VFS entry");
        assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), path2);
    }

    /// Override must be per-key: files unique to an earlier dir must survive
    /// even when later dirs override other keys.
    #[test]
    fn override_is_per_key_not_per_directory() {
        let dir1 = TempDir::new("vfsprio_perkey_dir1");
        let dir2 = TempDir::new("vfsprio_perkey_dir2");
        let keep = dir1.write("unique_to_dir1.txt", b"keep");
        dir1.write("shared.txt", b"dir1");
        dir2.write("shared.txt", b"dir2");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

        assert_eq!(vfs.iter().count(), 2);
        assert_eq!(vfs.get_file("unique_to_dir1.txt").unwrap().path(), keep);
    }

    /// Lookup with backslash separator must resolve to the same entry as
    /// forward-slash, and priority still applies.
    #[test]
    fn backslash_lookup_finds_overriding_file() {
        let dir1 = TempDir::new("vfsprio_bslash_dir1");
        let dir2 = TempDir::new("vfsprio_bslash_dir2");
        dir1.write("meshes/xbase.nif", b"dir1");
        let path2 = dir2.write("meshes/xbase.nif", b"dir2");

        let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

        assert_eq!(vfs.iter().count(), 1);
        assert_eq!(vfs.get_file("meshes\\xbase.nif").unwrap().path(), path2);
    }

    // ---- paths_matching ----

    #[test]
    fn paths_matching_finds_by_substring() {
        let dir = TempDir::new("vfsloose_matching");
        dir.write("textures/landscape/foo.dds", b"");
        dir.write("textures/sky/bar.dds", b"");
        dir.write("meshes/actors/baz.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert_eq!(vfs.paths_matching("textures").count(), 2);
        assert_eq!(vfs.paths_matching("meshes").count(), 1);
    }

    #[test]
    fn paths_matching_normalizes_query() {
        let dir = TempDir::new("vfsloose_matching_case");
        dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        // Uppercase query normalized before matching
        assert_eq!(vfs.paths_matching("TEXTURES").count(), 1);
        assert_eq!(vfs.paths_matching("Textures").count(), 1);
    }

    #[test]
    fn paths_matching_no_match_returns_empty() {
        let dir = TempDir::new("vfsloose_matching_empty");
        dir.write("meshes/foo.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert_eq!(vfs.paths_matching("textures").count(), 0);
    }

    // ---- paths_with ----

    #[test]
    fn paths_with_finds_all_under_prefix() {
        let dir = TempDir::new("vfsloose_with");
        dir.write("textures/landscape/a.dds", b"");
        dir.write("textures/landscape/b.dds", b"");
        dir.write("textures/sky/c.dds", b"");
        dir.write("meshes/foo.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        assert_eq!(vfs.paths_with("textures").count(), 3);
        assert_eq!(vfs.paths_with("textures/landscape").count(), 2);
        assert_eq!(vfs.paths_with("meshes").count(), 1);
    }

    #[test]
    fn paths_with_returns_empty_for_nonexistent_prefix() {
        let dir = TempDir::new("vfsloose_with_none");
        dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert_eq!(vfs.paths_with("sounds").count(), 0);
    }

    // ---- has_normalized_file / has_normalized_not_exact ----

    #[test]
    fn has_normalized_file_true_for_absolute_path_in_vfs() {
        let dir = TempDir::new("vfsloose_hasnorm_true");
        dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let abs = dir.path().join("textures/foo.dds");
        assert!(vfs.has_normalized_file(&abs));
    }

    #[test]
    fn has_normalized_file_false_for_unknown_path() {
        let dir = TempDir::new("vfsloose_hasnorm_false");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(!vfs.has_normalized_file(Path::new("/nonexistent/path/foo.dds")));
    }

    #[test]
    fn has_normalized_not_exact_true_when_path_exists_under_different_source() {
        // filtered_vfs has dir1/foo.txt at key "foo.txt"
        // has_normalized_not_exact(dir2/foo.txt) should be true:
        // dir2/foo.txt != dir1/foo.txt, but normalized dir2/foo.txt ends with "foo.txt"
        let dir1 = TempDir::new("vfsloose_notexact_dir1");
        let dir2 = TempDir::new("vfsloose_notexact_dir2");
        dir1.write("foo.txt", b"original");
        dir2.write("foo.txt", b"replacement");

        let filtered_vfs = VFS::from_directories(vec![dir1.path()], None);
        let path_from_dir2 = dir2.path().join("foo.txt");

        assert!(filtered_vfs.has_normalized_not_exact(&path_from_dir2));
    }

    #[test]
    fn has_normalized_not_exact_false_when_path_is_its_own_source() {
        let dir = TempDir::new("vfsloose_notexact_self");
        dir.write("foo.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let path = dir.path().join("foo.txt");
        // The only entry for "foo.txt" IS this file — not "not exact", so false
        assert!(!vfs.has_normalized_not_exact(&path));
    }

    #[test]
    fn has_normalized_not_exact_false_for_completely_absent_path() {
        let dir = TempDir::new("vfsloose_notexact_absent");
        dir.write("foo.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(!vfs.has_normalized_not_exact(Path::new("/some/other/dir/bar.txt")));
    }

    // ---- diff_directory ----

    #[test]
    fn diff_empty_dir_against_populated_vfs_yields_no_results() {
        let vfs_dir = TempDir::new("vfsdiff_base");
        vfs_dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

        let empty = TempDir::new("vfsdiff_empty_mod");
        let diff = vfs.diff_directory(empty.path());
        assert!(diff.conflicts.is_empty());
        assert!(diff.additions.is_empty());
    }

    #[test]
    fn diff_dir_with_only_new_files_yields_only_additions() {
        let vfs_dir = TempDir::new("vfsdiff_newfiles_base");
        vfs_dir.write("textures/vanilla.dds", b"");
        let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

        let mod_dir = TempDir::new("vfsdiff_newfiles_mod");
        let new1 = mod_dir.write("meshes/new_mesh.nif", b"");
        let new2 = mod_dir.write("textures/new_tex.dds", b"");

        let diff = vfs.diff_directory(mod_dir.path());
        assert!(diff.conflicts.is_empty());
        assert_eq!(diff.additions.len(), 2);

        let addition_paths: Vec<_> = diff.additions.iter().map(|(_, f)| f.path().to_path_buf()).collect();
        assert!(addition_paths.contains(&new1));
        assert!(addition_paths.contains(&new2));
    }

    #[test]
    fn diff_dir_with_only_conflicting_files_yields_only_conflicts() {
        let vfs_dir = TempDir::new("vfsdiff_conflicts_base");
        let existing = vfs_dir.write("textures/shared.dds", b"original");
        let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

        let mod_dir = TempDir::new("vfsdiff_conflicts_mod");
        let replacement = mod_dir.write("textures/shared.dds", b"replacement");

        let diff = vfs.diff_directory(mod_dir.path());
        assert!(diff.additions.is_empty());
        assert_eq!(diff.conflicts.len(), 1);

        let (key, incoming, displaced) = &diff.conflicts[0];
        assert_eq!(key.as_os_str(), "textures/shared.dds");
        assert_eq!(incoming.path(), replacement);
        assert_eq!(displaced.path(), existing);
    }

    #[test]
    fn diff_dir_mixed_yields_correct_split() {
        let vfs_dir = TempDir::new("vfsdiff_mixed_base");
        let existing = vfs_dir.write("textures/shared.dds", b"original");
        vfs_dir.write("textures/untouched.dds", b"");
        let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

        let mod_dir = TempDir::new("vfsdiff_mixed_mod");
        let replacement = mod_dir.write("textures/shared.dds", b"mod_version");
        let addition = mod_dir.write("meshes/new.nif", b"");

        let diff = vfs.diff_directory(mod_dir.path());

        assert_eq!(diff.conflicts.len(), 1);
        assert_eq!(diff.additions.len(), 1);

        let (_, incoming, displaced) = &diff.conflicts[0];
        assert_eq!(incoming.path(), replacement);
        assert_eq!(displaced.path(), existing);

        assert_eq!(diff.additions[0].1.path(), addition);
    }

    /// A file whose case/separators differ between dir and VFS must still be
    /// detected as a conflict — normalization applies to both sides.
    #[test]
    fn diff_dir_conflict_detected_across_case_normalization() {
        let vfs_dir = TempDir::new("vfsdiff_case_base");
        vfs_dir.write("textures/foo.dds", b"lowercase");
        let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

        let mod_dir = TempDir::new("vfsdiff_case_mod");
        // Mod stores the file with mixed case — same VFS key after normalization
        let replacement = mod_dir.write("Textures/Foo.DDS", b"mod_version");

        let diff = vfs.diff_directory(mod_dir.path());
        assert_eq!(diff.conflicts.len(), 1, "case variant should be detected as conflict");
        assert_eq!(diff.conflicts[0].1.path(), replacement);
    }

    /// Deeply nested files and subdirectories are all classified correctly.
    #[test]
    fn diff_dir_handles_deep_nesting() {
        let vfs_dir = TempDir::new("vfsdiff_deep_base");
        vfs_dir.write("a/b/c/d/deep.txt", b"");
        let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

        let mod_dir = TempDir::new("vfsdiff_deep_mod");
        let replacement = mod_dir.write("a/b/c/d/deep.txt", b"mod");
        let addition = mod_dir.write("a/b/c/d/new.txt", b"new");

        let diff = vfs.diff_directory(mod_dir.path());
        assert_eq!(diff.conflicts.len(), 1);
        assert_eq!(diff.additions.len(), 1);
        assert_eq!(diff.conflicts[0].1.path(), replacement);
        assert_eq!(diff.additions[0].1.path(), addition);
    }

    /// diff_directory against an empty VFS: everything is an addition.
    #[test]
    fn diff_dir_against_empty_vfs_yields_all_additions() {
        let empty_base = TempDir::new("vfsdiff_emptyvfs_base");
        let vfs = VFS::from_directories(vec![empty_base.path()], None);

        let mod_dir = TempDir::new("vfsdiff_emptyvfs_mod");
        mod_dir.write("a.txt", b"");
        mod_dir.write("b.txt", b"");

        let diff = vfs.diff_directory(mod_dir.path());
        assert!(diff.conflicts.is_empty());
        assert_eq!(diff.additions.len(), 2);
    }

    // ---- tree structure ----

    #[test]
    fn tree_relative_root_key_is_data_files() {
        let dir = TempDir::new("vfsloose_tree_relroot");
        dir.write("foo.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree(true);
        assert!(tree.contains_key(&PathBuf::from("Data Files")));
    }

    #[test]
    fn tree_absolute_root_key_is_slash() {
        let dir = TempDir::new("vfsloose_tree_absroot");
        dir.write("foo.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree(false);
        assert!(tree.contains_key(&PathBuf::from("/")));
    }

    #[test]
    fn tree_root_level_file_appears_in_root_node() {
        let dir = TempDir::new("vfsloose_tree_rootfile");
        dir.write("morrowind.esm", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree(true);
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();
        assert_eq!(root.files.len(), 1);
        assert_eq!(root.files[0].file_name().unwrap(), "morrowind.esm");
    }

    #[test]
    fn tree_nested_file_reachable_somewhere_in_tree() {
        let dir = TempDir::new("vfsloose_tree_nested");
        dir.write("textures/landscape/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree(true);
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();
        let all = collect_all_filenames(root);
        assert!(all.contains(&"foo.dds".to_string()));
    }

    #[test]
    fn tree_files_sorted_within_node() {
        let dir = TempDir::new("vfsloose_tree_sorted");
        dir.write("zoo.txt", b"");
        dir.write("alpha.txt", b"");
        dir.write("middle.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree(true);
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();
        let names: Vec<_> = root
            .files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "files within a DirectoryNode should be alphabetically sorted");
    }

    #[test]
    fn tree_subdir_keys_are_component_names_not_full_paths() {
        let dir = TempDir::new("vfsloose_tree_keys");
        dir.write("textures/landscape/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree(true);
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();

        assert!(
            root.subdirs.contains_key(&PathBuf::from("textures")),
            "top-level subdir should have key 'textures'"
        );
        let textures = root.subdirs.get(&PathBuf::from("textures")).unwrap();
        assert!(
            textures.subdirs.contains_key(&PathBuf::from("landscape")),
            "subdir key should be 'landscape', not 'textures/landscape' — see IMPROVEMENTS.md #14"
        );
    }

    // ---- tree_filtered ----

    #[test]
    fn tree_filtered_keeps_only_matching_files() {
        let dir = TempDir::new("vfsloose_filtered_keep");
        dir.write("textures/foo.dds", b"");
        dir.write("meshes/bar.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        let tree = vfs.tree_filtered(true, |file| {
            file.path().extension().is_some_and(|e| e == "dds")
        });
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();
        let all = collect_all_filenames(root);

        assert!(all.iter().all(|f| f.ends_with(".dds")), "only .dds files should survive the filter");
        assert!(!all.is_empty());
    }

    #[test]
    fn tree_filtered_prunes_empty_subdirs() {
        let dir = TempDir::new("vfsloose_filtered_prune");
        dir.write("textures/foo.dds", b"");
        dir.write("meshes/bar.nif", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        // Keep only .dds — the entire meshes/ subtree should disappear
        let tree = vfs.tree_filtered(true, |file| {
            file.path().extension().is_some_and(|e| e == "dds")
        });
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();

        fn contains_nif(node: &DirectoryNode) -> bool {
            node.files.iter().any(|f| f.path().extension().is_some_and(|e| e == "nif"))
                || node.subdirs.values().any(contains_nif)
        }
        assert!(!contains_nif(root), "empty subdirs should be pruned after filtering");
    }

    #[test]
    fn tree_filtered_all_excluded_yields_empty_root() {
        let dir = TempDir::new("vfsloose_filtered_all_gone");
        dir.write("foo.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let tree = vfs.tree_filtered(true, |_| false);
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();
        assert!(root.files.is_empty());
        assert!(root.subdirs.is_empty());
    }

    #[test]
    fn tree_filtered_all_included_matches_full_tree() {
        let dir = TempDir::new("vfsloose_filtered_all_in");
        dir.write("a/foo.txt", b"");
        dir.write("b/bar.txt", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        let full = vfs.tree(true);
        let filtered = vfs.tree_filtered(true, |_| true);

        let full_root = full.get(&PathBuf::from("Data Files")).unwrap();
        let filt_root = filtered.get(&PathBuf::from("Data Files")).unwrap();

        assert_eq!(
            collect_all_filenames(full_root),
            collect_all_filenames(filt_root),
        );
    }
}

#[cfg(all(test, feature = "bsa"))]
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
