// SPDX-License-Identifier: GPL-3.0-only
use crate::{
    LayerIndex, SourceKind, SourceMeta, normalize_path_in_place, paths::normalized_safe_key,
};
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Conflict information for a single source (directory or archive) within a load order.
///
/// A source has a **green** (override) indicator when it overrides at least
/// one file from an earlier source. It has a **red** (overridden) indicator
/// when at least one of its files is superseded by a later source.
///
/// Both can be true simultaneously for sources in the middle of the order.
#[derive(Debug, Default)]
pub struct SourceConflicts {
    /// Normalized VFS paths where this source wins over at least one earlier
    /// source (green up-arrow in MO2 terms).
    pub overrides: AHashSet<PathBuf>,

    /// Normalized VFS paths where this source loses to at least one later
    /// source (red down-arrow in MO2 terms).
    pub overridden_by: AHashSet<PathBuf>,
}

impl SourceConflicts {
    /// True if this source overrides at least one file from an earlier source.
    #[must_use]
    pub fn has_overrides(&self) -> bool {
        !self.overrides.is_empty()
    }

    /// True if at least one of this source's files is overridden by a later source.
    #[must_use]
    pub fn is_overridden(&self) -> bool {
        !self.overridden_by.is_empty()
    }
}

/// Full conflict analysis across an ordered list of sources (directories and/or archives).
///
/// Computes green/red conflict indicators for every source in a load order in
/// a single pass — no intermediate VFS builds required.
///
/// # Priority ordering
///
/// Matches `OpenMW`'s `data=` semantics: later sources in the list have higher
/// priority and win on collision. `sources[N-1]` is the highest-priority source.
/// When archives are included via [`ConflictIndex::from_directories_with_archives`],
/// they occupy the lowest-priority positions (before all directories), matching
/// `OpenMW`'s rule that loose files always beat archive files.
///
/// # Example
///
/// ```no_run
/// use vfstool_lib::ConflictIndex;
///
/// let index = ConflictIndex::from_directories(vec![
///     "/Data Files",
///     "/mods/better_textures",
///     "/mods/hd_overhaul",
/// ]);
///
/// // Does "better_textures" override anything from "Data Files"?
/// assert!(index.conflicts[1].has_overrides());
///
/// // Is "better_textures" itself overridden by "hd_overhaul"?
/// // (only if hd_overhaul contains the same paths)
/// let _ = index.conflicts[1].is_overridden();
/// ```
pub struct ConflictIndex {
    /// The sources (directories or archive paths) in load order.
    /// `sources[i]` corresponds to `conflicts[i]`.
    pub sources: Vec<PathBuf>,

    /// Source metadata in load order.
    pub(super) source_meta: Vec<SourceMeta>,

    /// Per-source conflict info, indexed by load-order position.
    pub conflicts: Vec<SourceConflicts>,

    /// Count of unique normalized files per source.
    pub(super) source_file_counts: Vec<usize>,

    /// Multi-map: normalized path → source indices (ascending = lower priority first).
    /// Only paths present in two or more sources are included.
    ///
    /// Use [`ConflictIndex::sources_containing`] for safe access.
    path_to_sources: AHashMap<PathBuf, Vec<usize>>,
}

impl ConflictIndex {
    /// Derive conflict information from a full provider-chain index.
    #[must_use]
    pub fn from_layer_index(layer: &LayerIndex) -> Self {
        let sources = layer.sources.clone();
        let mut source_file_counts = vec![0; sources.len()];
        let mut path_to_sources: AHashMap<PathBuf, Vec<usize>> = AHashMap::new();

        for key in layer.keys() {
            let providers = layer.sources_containing(&key);
            for &source_idx in providers {
                source_file_counts[source_idx] += 1;
            }
            if providers.len() > 1 {
                path_to_sources.insert(key, providers.to_vec());
            }
        }

        Self::from_provider_map(sources, source_file_counts, path_to_sources)
    }

    fn from_provider_map(
        source_meta: Vec<SourceMeta>,
        source_file_counts: Vec<usize>,
        path_to_sources: AHashMap<PathBuf, Vec<usize>>,
    ) -> Self {
        let mut conflicts: Vec<SourceConflicts> = (0..source_meta.len())
            .map(|_| SourceConflicts::default())
            .collect();

        for (path, source_indices) in &path_to_sources {
            // source_indices is sorted ascending (low priority → high priority).
            // Any entry after the first overrides something earlier (green).
            // Any entry before the last is overridden by something later (red).
            for (pos, &src_idx) in source_indices.iter().enumerate() {
                if pos > 0 {
                    conflicts[src_idx].overrides.insert(path.clone());
                }
                if pos < source_indices.len() - 1 {
                    conflicts[src_idx].overridden_by.insert(path.clone());
                }
            }
        }

        let sources = source_meta
            .iter()
            .map(|source| source.path.clone())
            .collect();

        Self {
            sources,
            source_meta,
            conflicts,
            source_file_counts,
            path_to_sources,
        }
    }

    /// Walk a single directory and return normalized, materialization-safe relative paths.
    pub(super) fn walk_dir(dir: &Path) -> Vec<PathBuf> {
        WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok().filter(|e| e.file_type().is_file()))
            .par_bridge()
            .filter_map(|entry| {
                let relative = entry
                    .path()
                    .strip_prefix(dir)
                    .expect("entry must be prefixed by scan dir")
                    .to_path_buf();
                normalized_safe_key(&relative)
            })
            .collect()
    }

    /// Core constructor: build a [`ConflictIndex`] from an ordered sequence of
    /// `(source_path, file_list)` pairs. Lower indices = lower priority.
    ///
    /// This is the low-level entry point for callers that have already assembled
    /// their file lists through other means (e.g. querying a game database, or
    /// mixing directory walks with archive enumeration at a layer above this crate).
    /// [`ConflictIndex::from_directories`] and [`ConflictIndex::from_directories_with_archives`]
    /// are thin wrappers around this function.
    pub fn from_file_lists(sources: impl IntoIterator<Item = (PathBuf, Vec<PathBuf>)>) -> Self {
        let layer = LayerIndex::from_file_lists(sources.into_iter().map(|(path, files)| {
            (
                SourceMeta {
                    path,
                    kind: SourceKind::LooseDir,
                },
                files,
            )
        }));
        Self::from_layer_index(&layer)
    }

    /// Analyse an ordered list of directories for VFS conflicts.
    ///
    /// Each directory is walked in parallel internally. The merge across
    /// directories is sequential to preserve load-order priority. A single
    /// pass over all files suffices — O(total files), no O(N²) partial builds.
    pub fn from_directories(dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>) -> Self {
        let sources: Vec<(PathBuf, Vec<PathBuf>)> = dirs
            .into_iter()
            .map(|d| {
                let d = d.as_ref().to_path_buf();
                let files = Self::walk_dir(&d);
                (d, files)
            })
            .collect();

        Self::from_file_lists(sources)
    }

    /// Returns the source indices (in load order, ascending priority) that
    /// contain `path`. Keys in the internal map are normalized; `path` is
    /// normalized before lookup.
    ///
    /// Returns an empty slice if the path appears in only one source (no
    /// conflict) or not at all.
    pub fn sources_containing(&self, path: &Path) -> &[usize] {
        let mut normalized = path.to_path_buf();
        normalize_path_in_place(&mut normalized);
        self.path_to_sources
            .get(&normalized)
            .map_or(&[], Vec::as_slice)
    }

    /// For a given source index and a conflicting path, returns the index of
    /// the source that `source_index`'s version of `path` directly displaces —
    /// i.e., the next-lower-priority source that also has this path.
    ///
    /// Returns `None` if `source_index` does not override anything for this path.
    #[must_use]
    pub fn displaced_by(&self, source_index: usize, path: &Path) -> Option<usize> {
        let indices = self.sources_containing(path);
        let pos = indices.iter().position(|&i| i == source_index)?;
        if pos == 0 {
            return None;
        }
        Some(indices[pos - 1])
    }

    /// For a given source index and a conflicting path, returns the index of
    /// the source that overrides `source_index`'s version of `path` —
    /// i.e., the next-higher-priority source that also has this path.
    ///
    /// Returns `None` if nothing later in the load order overrides this path.
    #[must_use]
    pub fn overridden_by_dir(&self, source_index: usize, path: &Path) -> Option<usize> {
        let indices = self.sources_containing(path);
        let pos = indices.iter().position(|&i| i == source_index)?;
        if pos == indices.len() - 1 {
            return None;
        }
        Some(indices[pos + 1])
    }
}
