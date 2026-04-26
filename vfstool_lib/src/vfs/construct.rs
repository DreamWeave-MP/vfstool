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

struct SourceEntries {
    source: SourceMeta,
    entries: Vec<(PathBuf, VfsFile)>,
}

impl VFS {
    #[cfg(any(feature = "bsa", feature = "zip"))]
    fn collect_archive_sources(
        loose_sources: &[SourceEntries],
        archive_list: Option<Vec<&str>>,
    ) -> Vec<SourceEntries> {
        let Some(list) = archive_list else {
            return Vec::new();
        };

        let loose_lookup: AHashMap<PathBuf, VfsFile> = loose_sources
            .iter()
            .flat_map(|source| {
                source
                    .entries
                    .iter()
                    .map(|(key, file)| (key.clone(), VfsFile::from(file.path())))
            })
            .collect();
        archives::from_set(&loose_lookup, &list)
            .iter()
            .map(|stored| {
                let archive_list = vec![Arc::clone(stored)];
                SourceEntries {
                    source: SourceMeta {
                        path: stored.path().to_path_buf(),
                        kind: SourceKind::Archive,
                    },
                    entries: archives::file_map(&archive_list).into_iter().collect(),
                }
            })
            .collect()
    }

    fn collect_loose_sources(dirs: Vec<PathBuf>) -> Vec<SourceEntries> {
        dirs.into_iter()
            .map(|dir| SourceEntries {
                entries: Self::directory_contents_to_file_map(&dir).collect(),
                source: SourceMeta {
                    path: dir,
                    kind: SourceKind::LooseDir,
                },
            })
            .collect()
    }

    fn append_sources(&mut self, sources: &[SourceEntries]) {
        for source in sources {
            let source_idx = self
                .provider_index
                .add_source(source.source.path.clone(), source.source.kind);
            for (key, file) in &source.entries {
                self.provider_index
                    .add_provider(source_idx, key.clone(), file);
            }
            self.file_map.extend(source.entries.iter().cloned());
        }
    }

    fn layer_sources_from(sources: &[SourceEntries]) -> Vec<(SourceMeta, Vec<PathBuf>)> {
        sources
            .iter()
            .map(|source| {
                (
                    source.source.clone(),
                    source
                        .entries
                        .iter()
                        .map(|(key, file)| {
                            if source.source.kind == SourceKind::LooseDir {
                                file.path()
                                    .strip_prefix(&source.source.path)
                                    .map_or_else(|_| key.clone(), Path::to_path_buf)
                            } else {
                                file.path().to_path_buf()
                            }
                        })
                        .collect(),
                )
            })
            .collect()
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

        let loose_sources = Self::collect_loose_sources(dirs);

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources = Self::collect_archive_sources(&loose_sources, archive_list);

        let mut vfs = Self::new();
        #[cfg(any(feature = "bsa", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        // Merge directories in order: later directories override earlier ones,
        // matching OpenMW's VFS semantics (last data= entry wins).
        vfs.append_sources(&loose_sources);

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

        let loose_sources = Self::collect_loose_sources(dirs);
        let dir_sources = Self::layer_sources_from(&loose_sources);

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources = Self::collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "bsa", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        // Archives occupy lowest-priority positions (prepended before directories).
        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = Self::layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        let conflict_index = ConflictIndex::from_layer_index(&layer_index);
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

        let loose_sources = Self::collect_loose_sources(dirs);
        let dir_sources = Self::layer_sources_from(&loose_sources);

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources = Self::collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "bsa", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = Self::layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        (vfs, layer_index)
    }
}
