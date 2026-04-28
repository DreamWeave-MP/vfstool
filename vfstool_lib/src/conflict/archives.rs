// SPDX-License-Identifier: GPL-3.0-only
use super::ConflictIndex;
use crate::{LayerIndex, SourceKind, SourceMeta};
use std::path::{Path, PathBuf};

impl ConflictIndex {
    /// Extract normalized VFS paths from an archive (BSA, BA2, ZIP, or PK3).
    ///
    /// Returns an empty list on any failure (missing file, unknown format, read
    /// error), consistent with how `VFS::from_directories` treats bad archives.
    fn paths_from_archive(path: &Path) -> Vec<PathBuf> {
        crate::archives::open_archive(path)
            .as_deref()
            .map(crate::archives::archive_paths)
            .unwrap_or_default()
    }

    /// Analyse an ordered set of directories and archives for VFS conflicts.
    ///
    /// Archives occupy the lowest-priority positions in the index (inserted
    /// before all directories), matching `OpenMW`'s rule that loose files always
    /// beat archive files. Within the archive list, order is preserved —
    /// `archive_paths[0]` is the lowest-priority archive.
    ///
    /// # Arguments
    ///
    /// * `dirs` — Data directories in load order (lowest priority first).
    /// * `archive_paths` — Absolute paths to BSA/BA2 archive files, in priority
    ///   order (lowest first). Typically these are resolved from the
    ///   `fallback-archive=` entries in `openmw.cfg`.
    pub fn from_directories_with_archives(
        dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>,
        archive_paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Self {
        // Archives come first — they have lower priority than any loose file.
        let archive_sources: Vec<(SourceMeta, Vec<PathBuf>)> = archive_paths
            .into_iter()
            .map(|p| {
                let p = p.as_ref().to_path_buf();
                let files = Self::paths_from_archive(&p);
                (
                    SourceMeta {
                        path: p,
                        kind: SourceKind::Archive,
                    },
                    files,
                )
            })
            .collect();

        let dir_sources: Vec<(SourceMeta, Vec<PathBuf>)> = dirs
            .into_iter()
            .map(|d| {
                let d = d.as_ref().to_path_buf();
                let files = Self::walk_dir(&d);
                (
                    SourceMeta {
                        path: d,
                        kind: SourceKind::LooseDir,
                    },
                    files,
                )
            })
            .collect();

        let layer = LayerIndex::from_file_lists(archive_sources.into_iter().chain(dir_sources));
        Self::from_layer_index(&layer)
    }
}
