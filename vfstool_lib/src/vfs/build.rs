// SPDX-License-Identifier: GPL-3.0-only
#[cfg(any(feature = "beth-archives", feature = "zip"))]
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
#[cfg(any(feature = "beth-archives", feature = "zip"))]
use std::sync::Arc;
use walkdir::WalkDir;

#[cfg(any(feature = "beth-archives", feature = "zip"))]
use crate::archives;
use crate::{NormalizedPath, SourceKind, SourceMeta, VfsFile, paths::normalized_safe_key};
use std::path::{Path, PathBuf};

pub(super) struct SourceEntries {
    pub(super) source: SourceMeta,
    pub(super) entries: Vec<(NormalizedPath, VfsFile)>,
}

#[cfg(any(feature = "beth-archives", feature = "zip"))]
pub(super) fn collect_archive_sources(
    loose_sources: &[SourceEntries],
    archive_list: Option<Vec<&str>>,
) -> Vec<SourceEntries> {
    let Some(list) = archive_list else {
        return Vec::new();
    };

    let loose_lookup = archive_lookup(loose_sources, &list);
    archives::from_set(&loose_lookup, &list)
        .iter()
        .map(|stored| {
            let archive_list = vec![Arc::clone(stored)];
            SourceEntries {
                source: SourceMeta {
                    path: stored.path().to_path_buf(),
                    kind: SourceKind::Archive,
                },
                entries: archives::file_entries(&archive_list),
            }
        })
        .collect()
}

#[cfg(any(feature = "beth-archives", feature = "zip"))]
fn archive_lookup(
    loose_sources: &[SourceEntries],
    archive_list: &[&str],
) -> AHashMap<NormalizedPath, VfsFile> {
    let requested = archive_list
        .iter()
        .map(|archive| NormalizedPath::new(archive.as_bytes()))
        .collect::<AHashSet<_>>();
    loose_sources
        .iter()
        .flat_map(|source| {
            source
                .entries
                .iter()
                .filter(|(key, _)| requested.contains(key))
                .map(|(key, file)| (key.clone(), VfsFile::from(file.path())))
        })
        .collect()
}

pub(super) fn collect_loose_sources(dirs: Vec<PathBuf>, sort_entries: bool) -> Vec<SourceEntries> {
    dirs.into_iter()
        .map(|dir| {
            let mut entries: Vec<_> = directory_contents_to_file_map(&dir).collect();
            if sort_entries {
                sort_source_entries(&mut entries);
            }
            SourceEntries {
                entries,
                source: SourceMeta {
                    path: dir,
                    kind: SourceKind::LooseDir,
                },
            }
        })
        .collect()
}

fn sort_source_entries(entries: &mut [(NormalizedPath, VfsFile)]) {
    entries.sort_by(|(left_key, left_file), (right_key, right_file)| {
        left_key
            .as_bytes()
            .cmp(right_key.as_bytes())
            .then_with(|| left_file.path().cmp(right_file.path()))
    });
}

fn directory_contents_to_file_map<I: AsRef<Path> + Sync>(
    dir: I,
) -> impl ParallelIterator<Item = (NormalizedPath, VfsFile)> {
    let dir = dir.as_ref().to_path_buf();
    WalkDir::new(&dir)
        .follow_links(true)
        .into_iter()
        .filter_map(move |entry| match entry {
            Ok(entry) if entry.file_type().is_file() => Some(entry),
            Ok(_) | Err(_) => None,
        })
        .par_bridge()
        .filter_map(move |entry| {
            let path = entry.path();
            let target_path = path
                .strip_prefix(&dir)
                .expect("Entry path should always be prefixed by scan directory!");

            let normalized_path = normalized_safe_key(target_path)?;

            let vfs_file = VfsFile::from(path);
            Some((normalized_path, vfs_file))
        })
}
