// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
use super::build::{
    SourceEntries, collect_loose_sources, layer_sources_from, try_collect_loose_sources,
};
#[cfg(any(feature = "beth-archives", feature = "zip"))]
use super::build::{collect_archive_sources, try_collect_archive_sources};
use crate::{ConflictIndex, LayerIndex, VfsBuildError, VfsProvider};
use std::path::{Path, PathBuf};

#[cfg(not(any(feature = "beth-archives", feature = "zip")))]
fn reject_archive_list_without_archive_features(
    archive_list: Option<&[&str]>,
) -> Result<(), VfsBuildError> {
    let Some(first_archive) = archive_list.and_then(|archives| archives.first()) else {
        return Ok(());
    };
    Err(VfsBuildError::ArchiveLoad {
        archive: PathBuf::from(first_archive),
        message: "archive support is not enabled; enable the `beth-archives` or `zip` feature"
            .to_owned(),
    })
}

impl VFS {
    fn append_sources(&mut self, sources: &[SourceEntries]) {
        for source in sources {
            let source_index = self.push_source(source.source.clone());
            for (key, file) in &source.entries {
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
    /// This legacy constructor is best-effort: unreadable traversal entries and archives that cannot
    /// be resolved or opened are skipped. Use [`VFS::try_from_directories`] when partial VFS state is
    /// not acceptable.
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

    /// Strictly build a [`VFS`] from an ordered list of directories and an optional archive list.
    ///
    /// Unlike [`VFS::from_directories`], this returns [`VfsBuildError`] if directory traversal fails,
    /// a configured archive is missing, or a configured archive cannot be opened.
    ///
    /// # Errors
    ///
    /// Returns [`VfsBuildError::Traversal`] for directory walk failures,
    /// [`VfsBuildError::ArchiveNotFound`] when a configured archive is not present in scanned loose
    /// sources, or [`VfsBuildError::ArchiveLoad`] when an archive is present but cannot be opened.
    pub fn try_from_directories(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(
            not(any(feature = "beth-archives", feature = "zip")),
            allow(unused_variables)
        )]
        archive_list: Option<Vec<&str>>,
    ) -> Result<Self, VfsBuildError> {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let loose_sources = try_collect_loose_sources(dirs, true)?;
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = try_collect_archive_sources(&loose_sources, archive_list)?;
        #[cfg(not(any(feature = "beth-archives", feature = "zip")))]
        reject_archive_list_without_archive_features(archive_list.as_deref())?;

        let mut vfs = Self::new();
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }
        vfs.append_sources(&loose_sources);
        Ok(vfs)
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
    /// This legacy constructor is best-effort. Use
    /// [`VFS::try_from_directories_with_conflict_index`] to make traversal or configured archive
    /// failures explicit.
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
        let dir_sources = layer_sources_from(&loose_sources);
        let mut vfs = Self::new();

        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = collect_archive_sources(&loose_sources, archive_list);
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        // Archives occupy lowest-priority positions (prepended before directories).
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let all_sources = layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "beth-archives", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        let conflict_index = ConflictIndex::from_layer_index(&layer_index);
        vfs.layer_index = Some(layer_index);
        (vfs, conflict_index)
    }

    /// Strictly build a [`VFS`] and [`ConflictIndex`] from the same scanned inputs.
    ///
    /// Returns [`VfsBuildError`] instead of silently dropping unreadable traversal entries or broken
    /// configured archives.
    ///
    /// # Errors
    ///
    /// Returns [`VfsBuildError::Traversal`] for directory walk failures,
    /// [`VfsBuildError::ArchiveNotFound`] when a configured archive is not present in scanned loose
    /// sources, or [`VfsBuildError::ArchiveLoad`] when an archive is present but cannot be opened.
    pub fn try_from_directories_with_conflict_index(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(
            not(any(feature = "beth-archives", feature = "zip")),
            allow(unused_variables)
        )]
        archive_list: Option<Vec<&str>>,
    ) -> Result<(Self, ConflictIndex), VfsBuildError> {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let loose_sources = try_collect_loose_sources(dirs, true)?;
        let dir_sources = layer_sources_from(&loose_sources);
        let mut vfs = Self::new();

        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = try_collect_archive_sources(&loose_sources, archive_list)?;
        #[cfg(not(any(feature = "beth-archives", feature = "zip")))]
        reject_archive_list_without_archive_features(archive_list.as_deref())?;
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);

        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let all_sources = layer_sources_from(&archive_sources)
            .into_iter()
            .chain(dir_sources)
            .collect::<Vec<_>>();
        #[cfg(not(any(feature = "beth-archives", feature = "zip")))]
        let all_sources = dir_sources;

        let layer_index = LayerIndex::from_file_lists(all_sources);
        let conflict_index = ConflictIndex::from_layer_index(&layer_index);
        vfs.layer_index = Some(layer_index);
        Ok((vfs, conflict_index))
    }

    /// Build a [`VFS`] and a [`LayerIndex`] from the same input sources.
    ///
    /// Unlike [`VFS::from_directories_with_conflict_index`], the returned
    /// [`LayerIndex`] contains provider chains for *all* keys (including unique
    /// keys with exactly one provider).
    ///
    /// This legacy constructor is best-effort. Use [`VFS::try_from_directories_with_layer_index`]
    /// to make traversal or configured archive failures explicit.
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
        vfs.layer_index = Some(layer_index.clone());
        (vfs, layer_index)
    }

    /// Strictly build a [`VFS`] and provider-occurrence-aware [`LayerIndex`] from the same inputs.
    ///
    /// Returns [`VfsBuildError`] instead of silently dropping unreadable traversal entries or broken
    /// configured archives.
    ///
    /// # Errors
    ///
    /// Returns [`VfsBuildError::Traversal`] for directory walk failures,
    /// [`VfsBuildError::ArchiveNotFound`] when a configured archive is not present in scanned loose
    /// sources, or [`VfsBuildError::ArchiveLoad`] when an archive is present but cannot be opened.
    pub fn try_from_directories_with_layer_index(
        search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        #[cfg_attr(
            not(any(feature = "beth-archives", feature = "zip")),
            allow(unused_variables)
        )]
        archive_list: Option<Vec<&str>>,
    ) -> Result<(Self, LayerIndex), VfsBuildError> {
        let dirs: Vec<PathBuf> = search_dirs
            .into_iter()
            .map(|d| d.as_ref().to_path_buf())
            .collect();

        let loose_sources = try_collect_loose_sources(dirs, true)?;

        let mut vfs = Self::new();

        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        let archive_sources = try_collect_archive_sources(&loose_sources, archive_list)?;
        #[cfg(not(any(feature = "beth-archives", feature = "zip")))]
        reject_archive_list_without_archive_features(archive_list.as_deref())?;
        #[cfg(any(feature = "beth-archives", feature = "zip"))]
        {
            vfs.append_sources(&archive_sources);
        }

        vfs.append_sources(&loose_sources);
        let layer_index = vfs.build_layer_index();
        vfs.layer_index = Some(layer_index.clone());
        Ok((vfs, layer_index))
    }
}
