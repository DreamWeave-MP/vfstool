// SPDX-License-Identifier: MIT OR Apache-2.0
use super::VFS;
#[cfg(any(feature = "bsa", feature = "zip"))]
use super::build::collect_archive_sources;
use super::build::{SourceEntries, collect_loose_sources, layer_sources_from};
use crate::{ConflictIndex, LayerIndex};
use std::path::{Path, PathBuf};

impl VFS {
    fn append_sources(&mut self, sources: &[SourceEntries]) {
        for source in sources {
            self.file_map.extend(source.entries.iter().cloned());
        }
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

        let loose_sources = collect_loose_sources(dirs);
        let dir_sources = layer_sources_from(&loose_sources);

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);

        let mut vfs = Self::new();
        #[cfg(any(feature = "bsa", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        // Merge directories in order: later directories override earlier ones,
        // matching OpenMW's VFS semantics (last data= entry wins).
        vfs.append_sources(&loose_sources);

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;
        vfs.layer_index = LayerIndex::from_file_lists(all_sources);

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

        let loose_sources = collect_loose_sources(dirs);
        let dir_sources = layer_sources_from(&loose_sources);

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "bsa", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        // Archives occupy lowest-priority positions (prepended before directories).
        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        let conflict_index = ConflictIndex::from_layer_index(&layer_index);
        vfs.layer_index = layer_index;
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

        let loose_sources = collect_loose_sources(dirs);
        let dir_sources = layer_sources_from(&loose_sources);

        let mut vfs = Self::new();

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "bsa", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        #[cfg(any(feature = "bsa", feature = "zip"))]
        let all_sources = layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "bsa", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        vfs.layer_index = layer_index.clone();
        (vfs, layer_index)
    }
}
