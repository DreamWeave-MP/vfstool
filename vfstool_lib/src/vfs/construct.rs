// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
#[cfg(any(feature = "beth-archives", feature = "zip"))]
use super::build::collect_archive_sources;
use super::build::{SourceEntries, collect_loose_sources};
use crate::{ConflictIndex, LayerIndex, VfsProvider};
use std::path::{Path, PathBuf};

impl VFS {
    fn append_sources(&mut self, sources: &[SourceEntries]) {
        for source in sources {
            let source_index = self.push_source(source.source.clone());
            for (key, file) in &source.entries {
                if !self.file_map.contains_key(key) && self.key_has_materialization_conflict(key) {
                    continue;
                }
                self.providers
                    .entry(key.clone())
                    .or_default()
                    .push(super::ProviderEntry {
                        source_index,
                        provider: VfsProvider {
                            source: source.source.clone(),
                            file: file.clone(),
                        },
                    });
                self.refresh_winner(key);
            }
        }
    }

    /// Build a [`VFS`] from an ordered list of directories and an optional archive list.
    ///
    /// Later entries in `search_dirs` override earlier ones (`OpenMW` `data=` semantics).
    /// If `archive_list` is provided and the `beth-archives` or `zip` feature is enabled, archives are
    /// loaded at lower priority than all loose files.
    ///
    /// Construction is best-effort but the returned VFS is always materializable: unreadable
    /// traversal entries, archives that cannot be resolved or opened, unsafe keys, and keys that
    /// would create file/directory materialization conflicts are skipped.
    pub fn from_directories(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(
            not(any(feature = "beth-archives", feature = "zip")),
            allow(unused_variables)
        )]
        archive_list: Option<Vec<&str>>,
    ) -> Self {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let loose_sources = collect_loose_sources(dirs, true);
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);

        let mut vfs = Self::new();
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
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
    ///
    /// Construction is best-effort. Invalid/unreadable inputs are skipped; the returned VFS and
    /// derived conflict index describe the valid provider set that was actually accepted.
    pub fn from_directories_with_conflict_index(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(
            not(any(feature = "beth-archives", feature = "zip")),
            allow(unused_variables)
        )]
        archive_list: Option<Vec<&str>>,
    ) -> (Self, ConflictIndex) {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let loose_sources = collect_loose_sources(dirs, true);
        let mut vfs = Self::new();

        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        let layer_index = vfs.build_layer_index();
        let conflict_index = ConflictIndex::from_layer_index(&layer_index);
        let _ = vfs.layer_index.set(layer_index);
        (vfs, conflict_index)
    }

    /// Build a [`VFS`] and a [`LayerIndex`] from the same input sources.
    ///
    /// Unlike [`VFS::from_directories_with_conflict_index`], the returned
    /// [`LayerIndex`] contains provider chains for *all* keys (including unique
    /// keys with exactly one provider).
    ///
    /// Construction is best-effort. Invalid/unreadable inputs are skipped; the returned layer index
    /// describes the valid provider set that was actually accepted.
    pub fn from_directories_with_layer_index(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(
            not(any(feature = "beth-archives", feature = "zip")),
            allow(unused_variables)
        )]
        archive_list: Option<Vec<&str>>,
    ) -> (Self, LayerIndex) {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let loose_sources = collect_loose_sources(dirs, true);

        let mut vfs = Self::new();

        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        let layer_index = vfs.build_layer_index();
        let _ = vfs.layer_index.set(layer_index.clone());
        (vfs, layer_index)
    }
}
