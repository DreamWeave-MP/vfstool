// SPDX-License-Identifier: MIT OR Apache-2.0
#[cfg(any(feature = "bsa", feature = "zip"))]
use ahash::AHashMap;
use rayon::prelude::*;
#[cfg(any(feature = "bsa", feature = "zip"))]
use std::sync::Arc;
use walkdir::WalkDir;

#[cfg(any(feature = "bsa", feature = "zip"))]
use crate::archives;
use crate::{SourceKind, SourceMeta, VfsFile, paths::normalized_safe_key};
use std::path::{Path, PathBuf};

pub(super) struct SourceEntries {
    pub(super) source: SourceMeta,
    pub(super) entries: Vec<(PathBuf, VfsFile)>,
}

#[cfg(any(feature = "bsa", feature = "zip"))]
pub(super) fn collect_archive_sources(
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

pub(super) fn collect_loose_sources(dirs: Vec<PathBuf>) -> Vec<SourceEntries> {
    dirs.into_iter()
        .map(|dir| SourceEntries {
            entries: directory_contents_to_file_map(&dir).collect(),
            source: SourceMeta {
                path: dir,
                kind: SourceKind::LooseDir,
            },
        })
        .collect()
}

pub(super) fn layer_sources_from(sources: &[SourceEntries]) -> Vec<(SourceMeta, Vec<PathBuf>)> {
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
