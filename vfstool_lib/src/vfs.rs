// SPDX-License-Identifier: MIT OR Apache-2.0
use ahash::AHashMap;
use rayon::prelude::*;
use walkdir::WalkDir;

#[cfg(feature = "serialize")]
use crate::SerializeType;
#[cfg(feature = "serialize")]
use std::io::{Error, ErrorKind, Result};

#[cfg(any(feature = "bsa", feature = "zip"))]
use crate::archives;

use crate::{
    ConflictIndex, DirectoryNode, DisplayTree, LayerIndex, SourceKind, SourceMeta, VfsFile,
    normalize_path, normalize_path_in_place,
};
use crate::reports::CollapseOptions;
use std::{
    collections::BTreeMap,
    fmt::Write,
    io,
    path::{Path, PathBuf},
};

// Owned
type MaybeFile<'a> = Option<&'a VfsFile>;
type VFSTuple<'a> = (&'a Path, &'a VfsFile);
type VFSFiles = AHashMap<PathBuf, VfsFile>;

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

/// Virtual file system built from an ordered list of data directories and optional archives.
///
/// Keys are normalized (lowercase, forward-slash) relative paths. Later directories and loose
/// files have higher priority, matching OpenMW's `data=` semantics.
pub struct VFS {
    file_map: VFSFiles,
}

impl VFS {
    const DIR_PREFIX: &str = "├── ";
    const FILE_PREFIX: &str = "│   ├── ";

    fn new() -> Self {
        Self {
            file_map: AHashMap::new(),
        }
    }

    /// Looks up a file in the VFS after normalizing the path.
    ///
    /// Already-normalized paths skip the allocation — the fast path is a
    /// direct `&Path` lookup with no heap activity.
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> MaybeFile<'_> {
        let p = path.as_ref();
        let bytes = p.as_os_str().as_encoded_bytes();
        if !bytes.iter().any(|&b| b == b'\\' || b.is_ascii_uppercase()) {
            self.file_map.get(p)
        } else {
            let normalized = normalize_path(p);
            self.file_map.get(&*normalized)
        }
    }

    /// Returns an iterator over all `(normalized_key, file)` pairs in the VFS.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &VfsFile)> {
        self.file_map.iter()
    }

    /// Returns a parallel iterator over all `(normalized_key, file)` pairs in the VFS.
    pub fn par_iter(&self) -> impl ParallelIterator<Item = (&PathBuf, &VfsFile)> {
        self.file_map.par_iter()
    }

    /// Dump every file in the VFS into `dir`, preserving relative paths.
    ///
    /// When `use_hardlinks` is `true`, loose files are hardlinked; cross-device
    /// link failures fall back to a copy. All other hardlink errors propagate.
    /// Archive files are always streamed via [`VfsFile::open`] regardless of mode.
    /// The destination directory must already exist. Returns the number of
    /// files successfully written.
    pub fn dump_to_directory(&self, dir: &Path, use_hardlinks: bool) -> std::io::Result<usize> {
        let written: std::io::Result<Vec<bool>> = self
            .file_map
            .par_iter()
            .map(|(relative_path, file)| -> std::io::Result<bool> {
                let dest = dir.join(relative_path);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if file.is_loose() {
                    if !file.path().exists() {
                        eprintln!(
                            "vfstool: skipping {}: source no longer exists at {}",
                            relative_path.display(),
                            file.path().display()
                        );
                        return Ok(false);
                    }
                    if use_hardlinks {
                        match std::fs::hard_link(file.path(), &dest) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                                std::fs::copy(file.path(), &dest)?;
                            }
                            Err(e) => return Err(e),
                        }
                    } else {
                        std::fs::copy(file.path(), &dest)?;
                    }
                } else {
                    match file.open() {
                        Ok(mut reader) => {
                            let mut out = std::fs::File::create(&dest)?;
                            std::io::copy(&mut reader, &mut out)?;
                        }
                        Err(e) => {
                            eprintln!("vfstool: skipping {}: {e}", relative_path.display());
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            })
            .collect();
        Ok(written?.into_iter().filter(|&ok| ok).count())
    }

    /// Search the VFS using a case-insensitive regex pattern.
    ///
    /// Returns a filtered [`DisplayTree`] containing only files whose VFS path
    /// matches `pattern`. The pattern is compiled with `case_insensitive(true)`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `pattern` is not a valid regex.
    pub fn find_by_regex(&self, pattern: &str, relative: bool) -> std::result::Result<DisplayTree, regex::Error> {
        let normalized = normalize_path(pattern);
        let re = regex::RegexBuilder::new(&normalized.to_string_lossy())
            .case_insensitive(true)
            .build()?;
        Ok(self.tree_filtered(relative, |_key, file| {
            let normalized_path = normalize_path(file.path());
            re.is_match(&normalized_path.to_string_lossy())
        }))
    }

    /// Collapse the entire VFS into `dest`, creating hardlinks, symlinks, or copies.
    ///
    /// Per-file errors are reported via `eprintln!` rather than aborting —
    /// this matches the original CLI behavior of continuing past individual
    /// link/copy failures.
    pub fn collapse_into(&self, dest: &Path, opts: &CollapseOptions) -> io::Result<()> {
        std::fs::create_dir_all(dest)?;

        self.file_map.iter().for_each(|(relative_path, file)| {
            let merged_path = dest.join(relative_path);
            let merged_dir = merged_path.parent().unwrap();

            if let Err(e) = std::fs::create_dir_all(merged_dir) {
                eprintln!(
                    "vfstool: failed to create directory {}: {}",
                    merged_dir.display(), e
                );
                return;
            }

            if file.is_loose() {
                if !file.path().exists() {
                    eprintln!(
                        "vfstool: skipping {}: source file no longer exists at {}",
                        relative_path.display(),
                        file.path().display()
                    );
                    return;
                }

                if let Err(e) = std::fs::remove_file(&merged_path)
                    && e.kind() != io::ErrorKind::NotFound
                {
                    eprintln!(
                        "vfstool: failed to remove existing file at {}: {}",
                        merged_path.display(), e
                    );
                    return;
                }

                // Skip archive files when --extract-archives is set
                if opts.extract_archives && let Some(ext) = file.path().extension() {
                    let ext = ext.to_ascii_lowercase();
                    let name = file.file_name().unwrap_or_default().to_ascii_lowercase();
                    if (ext == "bsa" || ext == "ba2")
                        && name != "archiveinvalidationinvalidated!.bsa"
                    {
                        eprintln!(
                            "vfstool: skipping archive {}",
                            file.file_name().unwrap().to_string_lossy()
                        );
                        return;
                    }
                }

                let link_result = if opts.use_symlinks {
                    Self::symlink(file.path(), &merged_path)
                } else {
                    std::fs::hard_link(file.path(), &merged_path)
                };

                if let Err(e) = link_result {
                    eprintln!(
                        "vfstool: link failed for {}: {}",
                        file.path().display(), e
                    );
                    if opts.allow_copying && let Err(e) = std::fs::copy(file.path(), &merged_path) {
                        eprintln!(
                            "vfstool: fallback copy of {} to {} failed: {}",
                            file.path().display(),
                            merged_path.display(),
                            e
                        );
                    }
                }
            } else if opts.extract_archives {
                match file.open() {
                    Ok(mut data) => {
                        use io::Read;
                        let mut buf = Vec::new();
                        match data.read_to_end(&mut buf) {
                            Err(e) => eprintln!(
                                "vfstool: failed to read archived file {}: {}",
                                relative_path.display(), e
                            ),
                            Ok(_) => {
                                if let Err(e) = std::fs::write(&merged_path, buf) {
                                    eprintln!(
                                        "vfstool: failed to extract {} to {}: {}",
                                        relative_path.display(),
                                        merged_path.display(),
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!(
                        "vfstool: failed to open archived file {}: {}",
                        relative_path.display(), e
                    ),
                }
            } else {
                eprintln!(
                    "vfstool: skipping {}, loaded from archive: {}",
                    relative_path.display(),
                    file.parent_archive_path().unwrap_or_default()
                );
            }
        });

        Ok(())
    }

    #[cfg(unix)]
    fn symlink(src: &Path, dst: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(src, dst)
    }

    #[cfg(windows)]
    fn symlink(src: &Path, dst: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(src, dst)
    }

    #[cfg(not(any(unix, windows)))]
    fn symlink(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks are not supported on this platform",
        ))
    }

    /// Extract a single VFS file into `dest_dir`.
    ///
    /// Returns the path of the extracted file on success. Returns `None` if
    /// `vfs_path` is not found in the VFS.
    pub fn extract_file(&self, vfs_path: &Path, dest_dir: &Path) -> io::Result<Option<PathBuf>> {
        let file = match self.get_file(vfs_path) {
            Some(f) => f,
            None => return Ok(None),
        };

        std::fs::create_dir_all(dest_dir)?;

        let file_name = vfs_path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "vfs_path has no file name"))?;

        let dest = dest_dir.join(file_name);

        if file.is_loose() {
            std::fs::copy(file.path(), &dest)?;
        } else {
            use io::Read;
            let mut buf = Vec::new();
            file.open()?.read_to_end(&mut buf)?;
            std::fs::write(&dest, buf)?;
        }

        Ok(Some(dest))
    }

    /// Return a filtered tree showing files from or replacing `filter_path`.
    ///
    /// `all_dirs` is the full ordered list of data directories (as from openmw.cfg).
    /// The method builds a single-directory VFS for `filter_path`, then filters
    /// the full VFS accordingly.
    ///
    /// When `replacements_only` is `false`: files still served from `filter_path`.
    /// When `replacements_only` is `true`: files where `filter_path` has a copy
    /// but the full VFS serves them from a different (higher-priority) source.
    pub fn remaining(
        &self,
        filter_path: &Path,
        replacements_only: bool,
        all_dirs: &[PathBuf],
        relative: bool,
    ) -> DisplayTree {
        let filter_normalized = normalize_path(filter_path).into_owned();

        let filtered_dirs: Vec<&PathBuf> = all_dirs
            .iter()
            .filter(|d| normalize_path(d.as_path()) == filter_normalized.as_path())
            .collect();

        let filtered_vfs = VFS::from_directories(filtered_dirs, None);

        self.tree_filtered(relative, |key, file| {
            if replacements_only {
                filtered_vfs.contains(key)
                    && !normalize_path(file.path()).starts_with(&filter_normalized)
            } else {
                normalize_path(file.path()).starts_with(&filter_normalized)
            }
        })
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
        let normalized_prefix = normalize_path(prefix.as_ref()).into_owned();
        self.file_map.iter().filter_map(move |(path, file)| {
            path.starts_with(&normalized_prefix).then_some((path.as_path(), file))
        })
    }

    /// Given a path prefix to a location in the VFS, return a parallel iterator to *all* of its contents.
    pub fn par_paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl ParallelIterator<Item = VFSTuple<'_>> {
        let normalized_prefix = normalize_path(prefix.as_ref()).into_owned();
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

    /// Build a [`VFS`] from an ordered list of directories and an optional archive list.
    ///
    /// Later entries in `search_dirs` override earlier ones (OpenMW `data=` semantics).
    /// If `archive_list` is provided and the `bsa` or `zip` feature is enabled, archives are
    /// loaded at lower priority than all loose files.
    pub fn from_directories(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(not(any(feature = "bsa", feature = "zip")), allow(unused_variables))]
        archive_list: Option<Vec<&str>>,
    ) -> Self {
        let mut vfs = Self::new();

        // Collect each dir as a Vec — rayon's parallel iterator collects into Vec
        // natively; AHashMap does not implement FromParallelIterator.
        let dir_entries: Vec<Vec<(PathBuf, VfsFile)>> = search_dirs
            .into_iter()
            .map(|dir| Self::directory_contents_to_file_map(dir).collect())
            .collect();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        if let Some(list) = archive_list {
            let loose_lookup: AHashMap<PathBuf, VfsFile> = dir_entries
                .iter()
                .flat_map(|entries| entries.iter().map(|(k, v)| (k.clone(), VfsFile::from(v.path()))))
                .collect();
            let archive_handles = archives::from_set(&loose_lookup, &list);
            vfs.file_map.extend(archives::file_map(archive_handles));
        }

        // Merge directories in order: later directories override earlier ones,
        // matching OpenMW's VFS semantics (last data= entry wins).
        for entries in dir_entries {
            vfs.file_map.extend(entries);
        }

        vfs
    }

    /// Build a [`VFS`] and a [`ConflictIndex`] from the same set of directories
    /// in a single directory walk.
    ///
    /// Equivalent to calling [`VFS::from_directories`] and
    /// [`ConflictIndex::from_directories_with_archives`] separately, but walks each
    /// directory only once. Both archives and loose files are reflected in the
    /// [`ConflictIndex`], with archives occupying lower-priority positions.
    ///
    /// # Priority ordering
    ///
    /// Matches OpenMW's `data=` semantics: later entries in `search_dirs` have
    /// higher priority. Archive sources appear before all directory sources in the
    /// `ConflictIndex` — index 0 is the lowest-priority archive (if any).
    pub fn from_directories_with_conflict_index(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(not(any(feature = "bsa", feature = "zip")), allow(unused_variables))]
        archive_list: Option<Vec<&str>>,
    ) -> (Self, ConflictIndex) {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        // Single walk per directory — results feed both VFS and ConflictIndex.
        let per_dir: Vec<Vec<(PathBuf, VfsFile)>> = dirs
            .iter()
            .map(|dir| Self::directory_contents_to_file_map(dir).collect())
            .collect();

        // Extract normalized keys for ConflictIndex before consuming per_dir.
        let conflict_sources: Vec<(PathBuf, Vec<PathBuf>)> = dirs
            .iter()
            .zip(per_dir.iter())
            .map(|(dir, entries)| {
                (dir.clone(), entries.iter().map(|(k, _)| k.clone()).collect())
            })
            .collect();

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_conflict_sources: Vec<(PathBuf, Vec<PathBuf>)> = {
            if let Some(list) = archive_list {
                let loose_lookup: AHashMap<PathBuf, VfsFile> = per_dir
                    .iter()
                    .flat_map(|entries| entries.iter().map(|(k, v)| (k.clone(), VfsFile::from(v.path()))))
                    .collect();
                let archive_handles = archives::from_set(&loose_lookup, &list);
                // Enumerate archive paths before consuming handles into file_map.
                let sources: Vec<(PathBuf, Vec<PathBuf>)> = archive_handles
                    .iter()
                    .map(|stored| (stored.path().to_path_buf(), archives::archive_paths(stored)))
                    .collect();
                vfs.file_map.extend(archives::file_map(archive_handles));
                sources
            } else {
                Vec::new()
            }
        };

        for entries in per_dir {
            vfs.file_map.extend(entries);
        }

        // Archives occupy lowest-priority positions (prepended before directories).
        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = archive_conflict_sources.into_iter().chain(conflict_sources).collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = conflict_sources;

        let conflict_index = ConflictIndex::from_file_lists(all_sources);
        (vfs, conflict_index)
    }

    /// Build a [`VFS`] and a [`LayerIndex`] from the same input sources.
    ///
    /// Unlike [`VFS::from_directories_with_conflict_index`], the returned
    /// [`LayerIndex`] contains provider chains for *all* keys (including unique
    /// keys with exactly one provider).
    pub fn from_directories_with_layer_index(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(not(any(feature = "bsa", feature = "zip")), allow(unused_variables))]
        archive_list: Option<Vec<&str>>,
    ) -> (Self, LayerIndex) {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let per_dir: Vec<Vec<(PathBuf, VfsFile)>> = dirs
            .iter()
            .map(|dir| Self::directory_contents_to_file_map(dir).collect())
            .collect();

        let dir_sources: Vec<(SourceMeta, Vec<PathBuf>)> = dirs
            .iter()
            .zip(per_dir.iter())
            .map(|(dir, entries)| {
                (
                    SourceMeta {
                        path: dir.clone(),
                        kind: SourceKind::LooseDir,
                    },
                    entries.iter().map(|(k, _)| k.clone()).collect(),
                )
            })
            .collect();

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources: Vec<(SourceMeta, Vec<PathBuf>)> = {
            if let Some(list) = archive_list {
                let loose_lookup: AHashMap<PathBuf, VfsFile> = per_dir
                    .iter()
                    .flat_map(|entries| entries.iter().map(|(k, v)| (k.clone(), VfsFile::from(v.path()))))
                    .collect();
                let archive_handles = archives::from_set(&loose_lookup, &list);
                let sources = archive_handles
                    .iter()
                    .map(|stored| {
                        (
                            SourceMeta {
                                path: stored.path().to_path_buf(),
                                kind: SourceKind::Archive,
                            },
                            archives::archive_paths(stored),
                        )
                    })
                    .collect();
                vfs.file_map.extend(archives::file_map(archive_handles));
                sources
            } else {
                Vec::new()
            }
        };

        for entries in per_dir {
            vfs.file_map.extend(entries);
        }

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = archive_sources.into_iter().chain(dir_sources).collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        (vfs, layer_index)
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

    /// Returns `true` if the VFS contains a file at `key`.
    ///
    /// `key` is a normalized relative VFS path (e.g. `"textures/foo.dds"`).
    /// The path is normalized before lookup, so case and separator variants
    /// are accepted. Already-normalized keys skip the allocation.
    pub fn contains(&self, key: &Path) -> bool {
        let bytes = key.as_os_str().as_encoded_bytes();
        if !bytes.iter().any(|&b| b == b'\\' || b.is_ascii_uppercase()) {
            self.file_map.contains_key(key)
        } else {
            let normalized = normalize_path(key);
            self.file_map.contains_key(&*normalized)
        }
    }

    /// Returns a sorted version of the VFS contents as a binary tree.
    pub fn tree(&self, relative: bool) -> DisplayTree {
        self.build_tree(relative, None::<fn(&Path, &VfsFile) -> bool>)
    }

    /// Returns a sorted tree containing only files accepted by `file_filter`.
    ///
    /// Unlike the old two-pass implementation (build full tree then prune),
    /// this filters during construction: directory nodes are only created for
    /// paths that contain at least one accepted file, so no separate prune pass
    /// is required.
    ///
    /// The predicate receives the normalized relative VFS key (`&Path`) and the
    /// `&VfsFile`. Having the key available allows O(1) cross-VFS lookups inside
    /// the predicate without needing to re-derive the relative path from the
    /// absolute physical path.
    pub fn tree_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&Path, &VfsFile) -> bool,
    ) -> DisplayTree {
        self.build_tree(relative, Some(file_filter))
    }

    fn build_tree<F: Fn(&Path, &VfsFile) -> bool>(
        &self,
        relative: bool,
        file_filter: Option<F>,
    ) -> DisplayTree {
        let mut tree: DisplayTree = BTreeMap::new();
        let root_path: PathBuf = if relative { "Data Files" } else { "/" }.into();

        tree.insert(root_path.clone(), DirectoryNode::new());

        for (key, entry) in &self.file_map {
            let path = if relative {
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
                );

            let new_file = match entry.is_archive() {
                false => VfsFile::from(entry.path()),
                #[cfg(any(feature = "bsa", feature = "zip"))]
                true => VfsFile::from_archive(
                    path.to_string_lossy(),
                    entry.parent_archive_handle().unwrap(),
                ),
                #[cfg(not(any(feature = "bsa", feature = "zip")))]
                true => unimplemented!(
                    "Archives are not supported in this build. Enable the 'bsa' or 'zip' feature of vfstool_lib to use them."
                ),
            };

            // Filter before touching the tree so we never create directory
            // nodes for paths whose files are all excluded.
            if file_filter.as_ref().is_some_and(|f| !f(key, &new_file)) {
                continue;
            }

            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(root_path.as_path());

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
                    .or_default();
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

    /// Returns the formatted file tree for a filtered subset
    pub fn display_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&Path, &VfsFile) -> bool,
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

fn write_node<W: Write>(w: &mut W, node: &DirectoryNode, dir: &Path) -> std::fmt::Result {
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

    // ---- contains ----

    #[test]
    fn contains_true_for_present_relative_key() {
        let dir = TempDir::new("vfsloose_contains_true");
        dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.contains(Path::new("textures/foo.dds")));
    }

    #[test]
    fn contains_normalizes_before_lookup() {
        let dir = TempDir::new("vfsloose_contains_norm");
        dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.contains(Path::new("Textures\\FOO.DDS")));
    }

    #[test]
    fn contains_false_for_absent_key() {
        let dir = TempDir::new("vfsloose_contains_false");
        dir.write("textures/foo.dds", b"");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(!vfs.contains(Path::new("textures/bar.dds")));
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

        let tree = vfs.tree_filtered(true, |_key, file| {
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
        let tree = vfs.tree_filtered(true, |_key, file| {
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
        let tree = vfs.tree_filtered(true, |_, _| false);
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
        let filtered = vfs.tree_filtered(true, |_, _| true);

        let full_root = full.get(&PathBuf::from("Data Files")).unwrap();
        let filt_root = filtered.get(&PathBuf::from("Data Files")).unwrap();

        assert_eq!(
            collect_all_filenames(full_root),
            collect_all_filenames(filt_root),
        );
    }

    // ---- find_by_regex ----

    fn count_files_in_tree(tree: &DisplayTree) -> usize {
        fn count(node: &DirectoryNode) -> usize {
            node.files.len() + node.subdirs.values().map(count).sum::<usize>()
        }
        tree.values().map(count).sum()
    }

    #[test]
    fn find_by_regex_matching_files_returned() {
        let dir = TempDir::new("vfs_newmethods_regex_match");
        dir.write("foo.txt", b"a");
        dir.write("bar.txt", b"b");
        dir.write("baz.nif", b"c");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        // Use a character-class instead of backslash-escaped dot so that
        // normalize_path (which converts '\' to '/') doesn't corrupt the pattern.
        let tree = vfs.find_by_regex("[.]txt$", true).unwrap();
        let count = count_files_in_tree(&tree);
        assert_eq!(count, 2, "only .txt files should match");
    }

    #[test]
    fn find_by_regex_non_matching_excluded() {
        let dir = TempDir::new("vfs_newmethods_regex_excl");
        dir.write("alpha.dds", b"a");
        dir.write("beta.nif", b"b");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        let tree = vfs.find_by_regex("[.]txt$", true).unwrap();
        let count = count_files_in_tree(&tree);
        assert_eq!(count, 0, "no .txt files should match");
    }

    #[test]
    fn find_by_regex_invalid_returns_err() {
        let dir = TempDir::new("vfs_newmethods_regex_err");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        assert!(vfs.find_by_regex(r"[invalid", true).is_err());
    }

    #[test]
    fn find_by_regex_case_insensitive() {
        let dir = TempDir::new("vfs_newmethods_regex_case");
        dir.write("foo.txt", b"a");
        dir.write("other.nif", b"b");
        let vfs = VFS::from_directories(vec![dir.path()], None);

        // Pattern in uppercase should still match the lowercase filename
        let tree = vfs.find_by_regex("FOO", true).unwrap();
        let count = count_files_in_tree(&tree);
        assert_eq!(count, 1, "FOO pattern should match foo.txt case-insensitively");
    }

    // ---- extract_file ----

    #[test]
    fn extract_file_returns_none_for_missing_path() {
        let dir = TempDir::new("vfs_newmethods_extract_none");
        let vfs = VFS::from_directories(vec![dir.path()], None);
        let dest = TempDir::new("vfs_newmethods_extract_none_dest");
        let result = vfs.extract_file(Path::new("nonexistent.txt"), dest.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn extract_file_copies_content_to_dest() {
        let src = TempDir::new("vfs_newmethods_extract_src");
        src.write("data.bin", b"hello extract");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("vfs_newmethods_extract_dest");
        let extracted = vfs
            .extract_file(Path::new("data.bin"), dest.path())
            .unwrap()
            .expect("file should be found and extracted");

        assert!(extracted.exists());
        assert_eq!(fs::read(&extracted).unwrap(), b"hello extract");
    }

    #[test]
    fn extract_file_creates_dest_dir_if_missing() {
        let src = TempDir::new("vfs_newmethods_extract_mkdest");
        src.write("note.txt", b"content");
        let vfs = VFS::from_directories(vec![src.path()], None);

        // Use a dest dir that does not yet exist
        let base = TempDir::new("vfs_newmethods_extract_mkdest_base");
        let dest = base.path().join("new_subdir");
        assert!(!dest.exists(), "dest should not exist yet");

        let extracted = vfs
            .extract_file(Path::new("note.txt"), &dest)
            .unwrap()
            .expect("file should be extracted");

        assert!(extracted.exists());
    }

    // ---- remaining ----

    #[test]
    fn remaining_replacements_false_includes_files_from_filter_path() {
        let dir1 = TempDir::new("vfs_newmethods_rem_d1");
        let dir2 = TempDir::new("vfs_newmethods_rem_d2");

        // dir1 has unique.txt (not overridden by dir2)
        dir1.write("unique.txt", b"from dir1");
        // both have shared.txt — dir2 wins
        dir1.write("shared.txt", b"dir1 version");
        dir2.write("shared.txt", b"dir2 version");
        dir2.write("only_dir2.txt", b"dir2 only");

        let all_dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        let vfs = VFS::from_directories(all_dirs.iter().map(|p| p.as_path()), None);

        // replacements_only=false: files still served FROM dir1
        let tree = vfs.remaining(dir1.path(), false, &all_dirs, true);
        let count = count_files_in_tree(&tree);
        // unique.txt is served from dir1; shared.txt is NOT (dir2 wins)
        assert_eq!(count, 1, "only unique.txt should still be served from dir1");
    }

    #[test]
    fn remaining_replacements_true_includes_overridden_files() {
        let dir1 = TempDir::new("vfs_newmethods_rem2_d1");
        let dir2 = TempDir::new("vfs_newmethods_rem2_d2");

        // dir1 has unique.txt and shared.txt; dir2 overrides shared.txt
        dir1.write("unique.txt", b"from dir1");
        dir1.write("shared.txt", b"dir1 version");
        dir2.write("shared.txt", b"dir2 version");

        let all_dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        let vfs = VFS::from_directories(all_dirs.iter().map(|p| p.as_path()), None);

        // replacements_only=true: files that dir1 has but a higher-priority dir wins
        let tree = vfs.remaining(dir1.path(), true, &all_dirs, true);
        let count = count_files_in_tree(&tree);
        // shared.txt is in dir1 but served from dir2 — it should appear
        // unique.txt is in dir1 and still served from dir1 — it should NOT appear
        assert_eq!(count, 1, "only the overridden shared.txt should appear");
    }
}

#[cfg(all(test, feature = "zip"))]
mod zip_tests {
    use super::*;
    use std::{
        fs,
        io::Write as IoWrite,
        path::{Path, PathBuf},
    };

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

        fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
            let target = self.0.join(rel);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, data).unwrap();
            target
        }

        /// Create a ZIP file at `filename` (relative to this dir) with the given entries.
        fn create_zip(&self, filename: &str, entries: &[(&str, &[u8])]) -> PathBuf {
            let path = self.0.join(filename);
            let file = fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // ---- Construction ----

    #[test]
    fn zip_entries_appear_in_vfs() {
        let dir = TempDir::new("vfszip_entries");
        dir.create_zip("data.zip", &[
            ("textures/foo.dds", b""),
            ("meshes/bar.nif", b""),
        ]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

        assert!(vfs.get_file("textures/foo.dds").is_some());
        assert!(vfs.get_file("meshes/bar.nif").is_some());
    }

    #[test]
    fn zip_entries_all_reachable() {
        // Verify all ZIP entries are in the VFS (the zip file itself also appears
        // as a loose entry since the data dir is walked, so we don't count total entries).
        let dir = TempDir::new("vfszip_count");
        dir.create_zip("data.zip", &[
            ("a.txt", b""),
            ("b.txt", b""),
            ("sub/c.txt", b""),
        ]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
        assert!(vfs.get_file("a.txt").is_some());
        assert!(vfs.get_file("b.txt").is_some());
        assert!(vfs.get_file("sub/c.txt").is_some());
    }

    // ---- open() / content ----

    #[test]
    fn zip_entry_content_readable() {
        let dir = TempDir::new("vfszip_content");
        dir.create_zip("data.zip", &[
            ("scripts/hello.lua", b"return 42"),
        ]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
        let file = vfs.get_file("scripts/hello.lua").unwrap();

        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
        assert_eq!(buf, b"return 42");
    }

    #[test]
    fn zip_entry_open_is_repeatable() {
        // open() must be callable multiple times (mutex lock, not move)
        let dir = TempDir::new("vfszip_repeat");
        dir.create_zip("data.zip", &[("foo.dat", b"hello")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
        let file = vfs.get_file("foo.dat").unwrap();

        for _ in 0..3 {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut file.open().unwrap(), &mut buf).unwrap();
            assert_eq!(buf, b"hello");
        }
    }

    // ---- Priority ----

    #[test]
    fn loose_file_overrides_zip_entry() {
        let dir = TempDir::new("vfszip_priority");
        dir.create_zip("data.zip", &[("textures/foo.dds", b"from_zip")]);
        let loose = dir.write("textures/foo.dds", b"from_loose");

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

        let file = vfs.get_file("textures/foo.dds").unwrap();
        assert!(file.is_loose(), "loose file must win over ZIP entry");
        assert_eq!(file.path(), loose);
    }

    #[test]
    fn later_dir_wins_over_zip_entry() {
        let archive_dir = TempDir::new("vfszip_prio_archive");
        archive_dir.create_zip("data.zip", &[("shared.txt", b"from_zip")]);

        let mod_dir = TempDir::new("vfszip_prio_mod");
        let loose = mod_dir.write("shared.txt", b"from_mod");

        let vfs = VFS::from_directories(
            vec![archive_dir.path(), mod_dir.path()],
            Some(vec!["data.zip"]),
        );

        let file = vfs.get_file("shared.txt").unwrap();
        assert_eq!(file.path(), loose, "loose dir entry must beat ZIP");
    }

    // ---- Flags ----

    #[test]
    fn zip_entry_is_archive_not_loose() {
        let dir = TempDir::new("vfszip_flag");
        dir.create_zip("data.zip", &[("meshes/cube.nif", b"")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
        let file = vfs.get_file("meshes/cube.nif").unwrap();

        assert!(file.is_archive());
        assert!(!file.is_loose());
    }

    #[test]
    fn zip_entry_parent_archive_name_matches_zip_filename() {
        let dir = TempDir::new("vfszip_archivename");
        dir.create_zip("mymod.zip", &[("icons/sword.dds", b"")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["mymod.zip"]));
        let file = vfs.get_file("icons/sword.dds").unwrap();

        assert_eq!(file.parent_archive_name().unwrap(), "mymod.zip");
    }

    // ---- Normalization ----

    #[test]
    fn zip_entry_case_insensitive_lookup() {
        let dir = TempDir::new("vfszip_case");
        dir.create_zip("data.zip", &[("textures/landscape/foo.dds", b"")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

        assert!(vfs.get_file("textures/landscape/foo.dds").is_some());
        assert!(vfs.get_file("Textures/Landscape/Foo.DDS").is_some());
        assert!(vfs.get_file("TEXTURES\\LANDSCAPE\\FOO.DDS").is_some());
    }

    #[test]
    fn zip_entry_uppercase_name_normalized_to_lowercase_key() {
        let dir = TempDir::new("vfszip_norm");
        // ZIPs from Windows tooling often have uppercase entry names.
        dir.create_zip("data.zip", &[("Meshes/Actors/XBase.NIF", b"nif_data")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));

        // Key must be lowercase after normalization
        assert!(vfs.get_file("meshes/actors/xbase.nif").is_some());

        // Content must still be readable despite the normalized lookup key
        let mut buf = Vec::new();
        std::io::Read::read_to_end(
            &mut vfs.get_file("meshes/actors/xbase.nif").unwrap().open().unwrap(),
            &mut buf,
        ).unwrap();
        assert_eq!(buf, b"nif_data");
    }

    // ---- PK3 ----

    #[test]
    fn pk3_extension_treated_as_zip() {
        let dir = TempDir::new("vfszip_pk3");
        dir.create_zip("pak0.pk3", &[("sound/ambient/wind.wav", b"wave_data")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["pak0.pk3"]));

        let file = vfs.get_file("sound/ambient/wind.wav").unwrap();
        assert!(file.is_archive());
        assert_eq!(file.parent_archive_name().unwrap(), "pak0.pk3");
    }

    // ---- Tree ----

    #[test]
    fn zip_entries_appear_in_tree() {
        let dir = TempDir::new("vfszip_tree");
        dir.create_zip("data.zip", &[("textures/sky.dds", b"")]);

        let vfs = VFS::from_directories(vec![dir.path()], Some(vec!["data.zip"]));
        let tree = vfs.tree(true);
        let root = tree.get(&PathBuf::from("Data Files")).unwrap();

        fn find_file(node: &DirectoryNode, name: &str) -> bool {
            node.files.iter().any(|f| f.file_name().is_some_and(|n| n == name))
                || node.subdirs.values().any(|sub| find_file(sub, name))
        }

        assert!(find_file(root, "sky.dds"), "ZIP entry should appear in tree");
    }
}

#[cfg(all(test, feature = "bsa"))]
mod tests {
    use super::*;
    use ba2::tes3::{Archive, ArchiveKey, File};
    use std::fs;
    use std::path::{Path, PathBuf};

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
        create_files(&temp_path.to_path_buf(), TEST_DATA);

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
        bsa1: &Path,
        bsa2: &Path,
        bsa3: &Path,
        dir1: &Path,
        dir2: &Path,
        dir3: &Path,
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

#[cfg(test)]
mod dump_tests {
    use super::*;
    use std::{fs, path::Path};

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

    #[test]
    fn dump_creates_files() {
        let src = TempDir::new("dump_creates_src");
        src.write("textures/foo.dds", b"dds_data");
        src.write("meshes/bar.nif", b"nif_data");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("dump_creates_dest");
        vfs.dump_to_directory(dest.path(), false).unwrap();

        assert_eq!(fs::read(dest.path().join("textures/foo.dds")).unwrap(), b"dds_data");
        assert_eq!(fs::read(dest.path().join("meshes/bar.nif")).unwrap(), b"nif_data");
    }

    #[test]
    fn dump_creates_subdirectories() {
        let src = TempDir::new("dump_subdirs_src");
        src.write("a/b/c/deep.txt", b"deep");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("dump_subdirs_dest");
        vfs.dump_to_directory(dest.path(), false).unwrap();

        assert!(dest.path().join("a/b/c/deep.txt").exists());
    }

    #[test]
    fn dump_count_accurate() {
        let src = TempDir::new("dump_count_src");
        src.write("a.txt", b"");
        src.write("b.txt", b"");
        src.write("sub/c.txt", b"");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("dump_count_dest");
        let count = vfs.dump_to_directory(dest.path(), false).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn dump_copy_mode() {
        let src = TempDir::new("dump_copy_src");
        src.write("data.txt", b"hello world");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("dump_copy_dest");
        vfs.dump_to_directory(dest.path(), false).unwrap();
        assert_eq!(fs::read(dest.path().join("data.txt")).unwrap(), b"hello world");
    }

    #[test]
    #[cfg(unix)]
    fn dump_hardlink_mode() {
        use std::os::unix::fs::MetadataExt;
        let src = TempDir::new("dump_hardlink_src");
        src.write("data.txt", b"linked");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("dump_hardlink_dest");
        vfs.dump_to_directory(dest.path(), true).unwrap();

        let meta = fs::metadata(dest.path().join("data.txt")).unwrap();
        assert!(meta.nlink() >= 2, "hardlinked file should have nlink >= 2");
    }

    #[test]
    fn dump_skips_missing_source() {
        let src = TempDir::new("dump_missing_src");
        let gone = src.write("gone.txt", b"x");
        let vfs = VFS::from_directories(vec![src.path()], None);

        // Delete source file before dumping
        fs::remove_file(&gone).unwrap();

        let dest = TempDir::new("dump_missing_dest");
        let count = vfs.dump_to_directory(dest.path(), false).unwrap();
        assert_eq!(count, 0);
        assert!(!dest.path().join("gone.txt").exists());
    }

    #[test]
    fn dump_overwrites_existing() {
        let src = TempDir::new("dump_overwrite_src");
        src.write("f.txt", b"new_content");
        let vfs = VFS::from_directories(vec![src.path()], None);

        let dest = TempDir::new("dump_overwrite_dest");
        dest.write("f.txt", b"old_content");

        vfs.dump_to_directory(dest.path(), false).unwrap();
        assert_eq!(fs::read(dest.path().join("f.txt")).unwrap(), b"new_content");
    }

    #[test]
    #[cfg(feature = "zip")]
    fn dump_archive_files_correct_content() {
        use std::io::Write as IoWrite;
        let src = TempDir::new("dump_zip_src");

        // Create a ZIP archive with one entry.
        let zip_path = src.path().join("data.zip");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("scripts/test.lua", options).unwrap();
            writer.write_all(b"return 42").unwrap();
            writer.finish().unwrap();
        }

        let vfs = VFS::from_directories(vec![src.path()], Some(vec!["data.zip"]));
        let dest = TempDir::new("dump_zip_dest");
        vfs.dump_to_directory(dest.path(), false).unwrap();

        let content = fs::read(dest.path().join("scripts/test.lua")).unwrap();
        assert_eq!(content, b"return 42");
    }
}
