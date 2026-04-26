// SPDX-License-Identifier: MIT OR Apache-2.0
use super::ConflictIndex;
use crate::{LayerIndex, SourceKind, SourceMeta};
use std::path::{Path, PathBuf};

impl ConflictIndex {
    #[cfg(feature = "zip")]
    fn zip_paths(path: &Path) -> Vec<PathBuf> {
        use std::fs::File;

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to open archive '{}': {e}",
                    path.display()
                );
                return Vec::new();
            }
        };

        match zip::ZipArchive::new(file) {
            Ok(archive) => archive
                .file_names()
                .filter(|name| !name.ends_with('/'))
                .filter_map(crate::archives::normalized_archive_key)
                .collect(),
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to read ZIP archive '{}': {e}",
                    path.display()
                );
                Vec::new()
            }
        }
    }

    #[cfg(feature = "bsa")]
    fn bsa_paths(path: &Path) -> Vec<PathBuf> {
        use ba2::prelude::*;
        use std::fs::File;

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "vfstool: warning: failed to open archive '{}': {e}",
                    path.display()
                );
                return Vec::new();
            }
        };

        let Some(format) = ba2::guess_format(&mut file) else {
            eprintln!(
                "vfstool: warning: could not determine format of archive '{}', skipping",
                path.display()
            );
            return Vec::new();
        };

        match format {
            ba2::FileFormat::TES3 => match ba2::tes3::Archive::read(&file) {
                Ok(archive) => archive
                    .iter()
                    .filter_map(|(key, _)| {
                        crate::archives::normalized_archive_key(&key.name().to_string())
                    })
                    .collect(),
                Err(e) => {
                    eprintln!(
                        "vfstool: warning: failed to read TES3 archive '{}': {e}",
                        path.display()
                    );
                    Vec::new()
                }
            },
            ba2::FileFormat::TES4 => match ba2::tes4::Archive::read(&file) {
                Ok((archive, _)) => archive
                    .iter()
                    .flat_map(|(dir_key, dir)| {
                        let dir_str = dir_key.name().to_string();
                        dir.iter()
                            .filter_map(move |(key, _)| {
                                crate::archives::normalized_archive_key(&format!(
                                    "{}\\{}",
                                    dir_str,
                                    key.name()
                                ))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect(),
                Err(e) => {
                    eprintln!(
                        "vfstool: warning: failed to read TES4 archive '{}': {e}",
                        path.display()
                    );
                    Vec::new()
                }
            },
            ba2::FileFormat::FO4 => match ba2::fo4::Archive::read(&file) {
                Ok((archive, _)) => archive
                    .iter()
                    .filter_map(|(key, _)| {
                        crate::archives::normalized_archive_key(&key.name().to_string())
                    })
                    .collect(),
                Err(e) => {
                    eprintln!(
                        "vfstool: warning: failed to read FO4 archive '{}': {e}",
                        path.display()
                    );
                    Vec::new()
                }
            },
        }
    }

    /// Extract normalized VFS paths from an archive (BSA, BA2, ZIP, or PK3).
    ///
    /// Logs a warning and returns an empty list on any failure (missing file,
    /// unknown format, read error), consistent with how `VFS::from_directories`
    /// treats bad archives.
    #[allow(unreachable_code)]
    fn paths_from_archive(path: &Path) -> Vec<PathBuf> {
        #[cfg(feature = "zip")]
        if crate::archives::is_zip_or_pk3(path) {
            return Self::zip_paths(path);
        }

        #[cfg(feature = "bsa")]
        return Self::bsa_paths(path);

        eprintln!(
            "vfstool: warning: '{}' is not a recognized archive format, skipping",
            path.display()
        );
        Vec::new()
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
