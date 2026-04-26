// SPDX-License-Identifier: MIT OR Apache-2.0
use super::VFS;
#[cfg(any(feature = "bsa", feature = "zip"))]
use ahash::AHashMap;
use rayon::prelude::*;
#[cfg(any(feature = "bsa", feature = "zip"))]
use std::sync::Arc;
use walkdir::WalkDir;

#[cfg(any(feature = "bsa", feature = "zip"))]
use crate::archives;
use crate::{
    ConflictIndex, LayerIndex, SourceKind, SourceMeta, VfsFile, paths::normalized_safe_key,
};
use std::path::{Path, PathBuf};

impl VFS {
    #[cfg(any(feature = "bsa", feature = "zip"))]
    fn add_archive_providers_to_index(&mut self, archive_handles: &archives::ArchiveList) {
        for stored in archive_handles {
            let source_idx = self
                .provider_index
                .add_source(stored.path().to_path_buf(), SourceKind::Archive);
            let archive_list = vec![Arc::clone(stored)];
            for (key, file) in archives::file_map(&archive_list) {
                self.provider_index.add_provider(source_idx, key, &file);
            }
        }
    }

    fn add_loose_providers_to_index(
        &mut self,
        dirs: &[PathBuf],
        per_dir: &[Vec<(PathBuf, VfsFile)>],
    ) {
        for (dir, entries) in dirs.iter().zip(per_dir) {
            let source_idx = self
                .provider_index
                .add_source(dir.clone(), SourceKind::LooseDir);
            for (key, file) in entries {
                self.provider_index
                    .add_provider(source_idx, key.clone(), file);
            }
        }
    }

    /// Returns a parallel iterator meant to be fed into `par_extend`
    /// Only used when appending a directory or set of directories into the file map
    fn directory_contents_to_file_map<I: AsRef<Path> + Sync>(
        dir: I,
    ) -> impl ParallelIterator<Item = (PathBuf, VfsFile)> {
        let dir = dir.as_ref().to_path_buf();
        let walk_root = dir.clone();

        WalkDir::new(&dir)
            .follow_links(true)
            .into_iter()
            .filter_map(move |entry| match entry {
                Ok(entry) if entry.file_type().is_file() => Some(entry),
                Ok(_) => None,
                Err(err) => {
                    eprintln!(
                        "vfstool: warning: failed to walk '{}': {err}",
                        walk_root.display()
                    );
                    None
                }
            })
            .par_bridge()
            .filter_map(move |entry| {
                let path = entry.path();
                let target_path = path
                    .strip_prefix(&dir)
                    .expect("Entry path should always be prefixed by scan directory!");

                let Some(normalized_path) = normalized_safe_key(target_path) else {
                    eprintln!(
                        "vfstool: skipping unsafe VFS path '{}' from {}",
                        target_path.display(),
                        path.display()
                    );
                    return None;
                };

                let vfs_file = VfsFile::from(path);
                Some((normalized_path, vfs_file))
            })
    }

    /// Build a [`VFS`] from an ordered list of directories and an optional archive list.
    ///
    /// Later entries in `search_dirs` override earlier ones (`OpenMW` `data=` semantics).
    /// If `archive_list` is provided and the `bsa` or `zip` feature is enabled, archives are
    /// loaded at lower priority than all loose files.
    pub fn from_directories(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(not(any(feature = "bsa", feature = "zip")), allow(unused_variables))]
        archive_list: Option<Vec<&str>>,
    ) -> Self {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let mut vfs = Self::new();

        // Collect each dir as a Vec — rayon's parallel iterator collects into Vec
        // natively; AHashMap does not implement FromParallelIterator.
        let dir_entries: Vec<Vec<(PathBuf, VfsFile)>> = dirs
            .iter()
            .map(|dir| Self::directory_contents_to_file_map(dir).collect())
            .collect();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        if let Some(list) = archive_list {
            let loose_lookup: AHashMap<PathBuf, VfsFile> = dir_entries
                .iter()
                .flat_map(|entries| {
                    entries
                        .iter()
                        .map(|(k, v)| (k.clone(), VfsFile::from(v.path())))
                })
                .collect();
            let archive_handles = archives::from_set(&loose_lookup, &list);
            vfs.add_archive_providers_to_index(&archive_handles);
            vfs.file_map.extend(archives::file_map(&archive_handles));
        }

        vfs.add_loose_providers_to_index(&dirs, &dir_entries);

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
    /// Matches `OpenMW`'s `data=` semantics: later entries in `search_dirs` have
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
                (
                    dir.clone(),
                    entries.iter().map(|(k, _)| k.clone()).collect(),
                )
            })
            .collect();

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_conflict_sources: Vec<(PathBuf, Vec<PathBuf>)> = {
            if let Some(list) = archive_list {
                let loose_lookup: AHashMap<PathBuf, VfsFile> = per_dir
                    .iter()
                    .flat_map(|entries| {
                        entries
                            .iter()
                            .map(|(k, v)| (k.clone(), VfsFile::from(v.path())))
                    })
                    .collect();
                let archive_handles = archives::from_set(&loose_lookup, &list);
                vfs.add_archive_providers_to_index(&archive_handles);
                // Enumerate archive paths before consuming handles into file_map.
                let sources: Vec<(PathBuf, Vec<PathBuf>)> = archive_handles
                    .iter()
                    .map(|stored| (stored.path().to_path_buf(), archives::archive_paths(stored)))
                    .collect();
                vfs.file_map.extend(archives::file_map(&archive_handles));
                sources
            } else {
                Vec::new()
            }
        };

        vfs.add_loose_providers_to_index(&dirs, &per_dir);

        for entries in per_dir {
            vfs.file_map.extend(entries);
        }

        // Archives occupy lowest-priority positions (prepended before directories).
        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = archive_conflict_sources
            .into_iter()
            .chain(conflict_sources)
            .collect::<Vec<_>>();
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
                    entries
                        .iter()
                        .map(|(key, file)| {
                            file.path()
                                .strip_prefix(dir)
                                .map_or_else(|_| key.clone(), Path::to_path_buf)
                        })
                        .collect(),
                )
            })
            .collect();

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources: Vec<(SourceMeta, Vec<PathBuf>)> = {
            if let Some(list) = archive_list {
                let loose_lookup: AHashMap<PathBuf, VfsFile> = per_dir
                    .iter()
                    .flat_map(|entries| {
                        entries
                            .iter()
                            .map(|(k, v)| (k.clone(), VfsFile::from(v.path())))
                    })
                    .collect();
                let archive_handles = archives::from_set(&loose_lookup, &list);
                vfs.add_archive_providers_to_index(&archive_handles);
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
                vfs.file_map.extend(archives::file_map(&archive_handles));
                sources
            } else {
                Vec::new()
            }
        };

        vfs.add_loose_providers_to_index(&dirs, &per_dir);

        for entries in per_dir {
            vfs.file_map.extend(entries);
        }

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = archive_sources
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        (vfs, layer_index)
    }
}
