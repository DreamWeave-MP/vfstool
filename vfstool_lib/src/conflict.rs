// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::normalize_path_in_place;
use ahash::{AHashMap, AHashSet};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

mod report_methods;

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

    /// Per-source conflict info, indexed by load-order position.
    pub conflicts: Vec<SourceConflicts>,

    /// Count of unique normalized files per source.
    source_file_counts: Vec<usize>,

    /// Multi-map: normalized path → source indices (ascending = lower priority first).
    /// Only paths present in two or more sources are included.
    ///
    /// Use [`ConflictIndex::sources_containing`] for safe access.
    path_to_sources: AHashMap<PathBuf, Vec<usize>>,
}

impl ConflictIndex {
    /// Walk a single directory and return normalized relative paths.
    fn walk_dir(dir: &Path) -> Vec<PathBuf> {
        WalkDir::new(dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok().filter(|e| e.file_type().is_file()))
            .par_bridge()
            .map(|entry| {
                let mut normalized = entry
                    .path()
                    .strip_prefix(dir)
                    .expect("entry must be prefixed by scan dir")
                    .to_path_buf();
                normalize_path_in_place(&mut normalized);
                normalized
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
        let mut source_paths: Vec<PathBuf> = Vec::new();
        let mut source_file_counts: Vec<usize> = Vec::new();
        let mut path_to_sources: AHashMap<PathBuf, Vec<usize>> = AHashMap::new();

        // Sequential merge preserves source order and therefore priority.
        for (source_path, files) in sources {
            let source_idx = source_paths.len();
            source_paths.push(source_path);
            let mut seen = AHashSet::new();
            for file in files {
                let mut normalized = file;
                normalize_path_in_place(&mut normalized);
                if seen.insert(normalized.clone()) {
                    path_to_sources
                        .entry(normalized)
                        .or_default()
                        .push(source_idx);
                }
            }
            source_file_counts.push(seen.len());
        }

        let n = source_paths.len();

        // Remove paths that appear in only one source — they have no conflict.
        path_to_sources.retain(|_, indices| indices.len() > 1);

        // Derive per-source winning/losing sets from the multi-map.
        let mut conflicts: Vec<SourceConflicts> =
            (0..n).map(|_| SourceConflicts::default()).collect();

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

        Self {
            sources: source_paths,
            conflicts,
            source_file_counts,
            path_to_sources,
        }
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

#[cfg(any(feature = "bsa", feature = "zip"))]
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
        let archive_sources: Vec<(PathBuf, Vec<PathBuf>)> = archive_paths
            .into_iter()
            .map(|p| {
                let p = p.as_ref().to_path_buf();
                let files = Self::paths_from_archive(&p);
                (p, files)
            })
            .collect();

        let dir_sources: Vec<(PathBuf, Vec<PathBuf>)> = dirs
            .into_iter()
            .map(|d| {
                let d = d.as_ref().to_path_buf();
                let files = Self::walk_dir(&d);
                (d, files)
            })
            .collect();

        Self::from_file_lists(archive_sources.into_iter().chain(dir_sources))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VFS;
    use std::{fs, path::Path};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::current_dir().unwrap().join(name);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, rel: &str, data: &[u8]) {
            let target = self.0.join(rel);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, data).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_overlap_no_conflicts() {
        let d1 = TempDir::new("ci_nooverlap_d1");
        let d2 = TempDir::new("ci_nooverlap_d2");
        d1.write("a.txt", b"");
        d2.write("b.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

        assert!(!index.conflicts[0].has_overrides());
        assert!(!index.conflicts[0].is_overridden());
        assert!(!index.conflicts[1].has_overrides());
        assert!(!index.conflicts[1].is_overridden());
    }

    #[test]
    fn later_dir_shows_green_earlier_dir_shows_red() {
        let d1 = TempDir::new("ci_greenred_d1");
        let d2 = TempDir::new("ci_greenred_d2");
        d1.write("shared.txt", b"");
        d2.write("shared.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

        // d1 (earlier, lower priority): overridden, not overriding
        assert!(!index.conflicts[0].has_overrides(), "d1 overrides nothing");
        assert!(index.conflicts[0].is_overridden(), "d1 is overridden by d2");

        // d2 (later, higher priority): overriding, not overridden
        assert!(index.conflicts[1].has_overrides(), "d2 overrides d1");
        assert!(!index.conflicts[1].is_overridden(), "nothing overrides d2");
    }

    #[test]
    fn middle_dir_shows_both_arrows() {
        let d1 = TempDir::new("ci_both_d1");
        let d2 = TempDir::new("ci_both_d2");
        let d3 = TempDir::new("ci_both_d3");
        d1.write("shared.txt", b"");
        d2.write("shared.txt", b"");
        d3.write("shared.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);

        assert!(!index.conflicts[0].has_overrides());
        assert!(index.conflicts[0].is_overridden());

        // d2 is in the middle: overrides d1, overridden by d3
        assert!(
            index.conflicts[1].has_overrides(),
            "d2 should have green arrow"
        );
        assert!(
            index.conflicts[1].is_overridden(),
            "d2 should have red arrow"
        );

        assert!(index.conflicts[2].has_overrides());
        assert!(!index.conflicts[2].is_overridden());
    }

    #[test]
    fn conflict_paths_are_normalized() {
        let d1 = TempDir::new("ci_norm_d1");
        let d2 = TempDir::new("ci_norm_d2");
        d1.write("Textures/Foo.DDS", b"");
        d2.write("textures/foo.dds", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

        // Despite different on-disk names, they normalize to the same key
        assert!(index.conflicts[1].has_overrides());
        assert!(index.conflicts[0].is_overridden());

        let key = PathBuf::from("textures/foo.dds");
        assert!(index.conflicts[1].overrides.contains(&key));
        assert!(index.conflicts[0].overridden_by.contains(&key));
    }

    #[test]
    fn unique_files_not_in_conflict_sets() {
        let d1 = TempDir::new("ci_unique_d1");
        let d2 = TempDir::new("ci_unique_d2");
        d1.write("shared.txt", b"");
        d1.write("only_in_d1.txt", b"");
        d2.write("shared.txt", b"");
        d2.write("only_in_d2.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

        let unique1 = PathBuf::from("only_in_d1.txt");
        let unique2 = PathBuf::from("only_in_d2.txt");
        assert!(!index.conflicts[0].overridden_by.contains(&unique1));
        assert!(!index.conflicts[1].overrides.contains(&unique2));
    }

    #[test]
    fn duplicate_keys_within_one_source_do_not_self_conflict() {
        let index = ConflictIndex::from_file_lists(vec![
            (
                PathBuf::from("/one"),
                vec![PathBuf::from("shared.txt"), PathBuf::from("shared.txt")],
            ),
            (PathBuf::from("/two"), vec![PathBuf::from("unique.txt")]),
        ]);

        assert!(index.sources_containing(Path::new("shared.txt")).is_empty());
        assert!(index.conflicts[0].overrides.is_empty());
        assert!(index.conflicts[0].overridden_by.is_empty());
    }

    #[test]
    fn from_file_lists_normalizes_caller_supplied_paths() {
        let index = ConflictIndex::from_file_lists(vec![
            (
                PathBuf::from("/one"),
                vec![PathBuf::from("Textures/Foo.DDS")],
            ),
            (
                PathBuf::from("/two"),
                vec![PathBuf::from("textures\\foo.dds")],
            ),
        ]);

        assert_eq!(
            index.sources_containing(Path::new("textures/foo.dds")),
            &[0, 1]
        );
        assert!(index.conflicts[1].has_overrides());
        assert!(index.conflicts[0].is_overridden());
    }

    #[test]
    fn sources_containing_returns_indices_in_order() {
        let d1 = TempDir::new("ci_dircontaining_d1");
        let d2 = TempDir::new("ci_dircontaining_d2");
        let d3 = TempDir::new("ci_dircontaining_d3");
        d1.write("shared.txt", b"");
        d2.write("shared.txt", b"");
        d3.write("shared.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);

        let indices = index.sources_containing(Path::new("shared.txt"));
        assert_eq!(indices, &[0, 1, 2]);
    }

    #[test]
    fn displaced_by_returns_next_lower_priority_dir() {
        let d1 = TempDir::new("ci_displaced_d1");
        let d2 = TempDir::new("ci_displaced_d2");
        let d3 = TempDir::new("ci_displaced_d3");
        d1.write("shared.txt", b"");
        d2.write("shared.txt", b"");
        d3.write("shared.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);
        let path = Path::new("shared.txt");

        // d3 (index 2) directly displaces d2 (index 1)
        assert_eq!(index.displaced_by(2, path), Some(1));
        // d2 (index 1) directly displaces d1 (index 0)
        assert_eq!(index.displaced_by(1, path), Some(0));
        // d1 (index 0) displaces nothing
        assert_eq!(index.displaced_by(0, path), None);
    }

    #[test]
    fn overridden_by_dir_returns_next_higher_priority_dir() {
        let d1 = TempDir::new("ci_overriddenby_d1");
        let d2 = TempDir::new("ci_overriddenby_d2");
        let d3 = TempDir::new("ci_overriddenby_d3");
        d1.write("shared.txt", b"");
        d2.write("shared.txt", b"");
        d3.write("shared.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);
        let path = Path::new("shared.txt");

        // d1 (index 0) is directly overridden by d2 (index 1)
        assert_eq!(index.overridden_by_dir(0, path), Some(1));
        // d2 (index 1) is directly overridden by d3 (index 2)
        assert_eq!(index.overridden_by_dir(1, path), Some(2));
        // d3 (index 2) is overridden by nothing
        assert_eq!(index.overridden_by_dir(2, path), None);
    }

    #[test]
    fn empty_directories_produce_no_conflicts() {
        let d1 = TempDir::new("ci_empty_d1");
        let d2 = TempDir::new("ci_empty_d2");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        assert!(!index.conflicts[0].has_overrides());
        assert!(!index.conflicts[0].is_overridden());
        assert!(!index.conflicts[1].has_overrides());
        assert!(!index.conflicts[1].is_overridden());
    }

    #[test]
    fn single_directory_never_conflicts_with_itself() {
        let d1 = TempDir::new("ci_single_d1");
        d1.write("a.txt", b"");
        d1.write("b.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path()]);
        assert!(!index.conflicts[0].has_overrides());
        assert!(!index.conflicts[0].is_overridden());
    }

    #[test]
    fn partial_overlap_correct_per_dir_counts() {
        let d1 = TempDir::new("ci_partial_d1");
        let d2 = TempDir::new("ci_partial_d2");
        d1.write("shared_a.txt", b"");
        d1.write("shared_b.txt", b"");
        d1.write("only_d1.txt", b"");
        d2.write("shared_a.txt", b"");
        d2.write("shared_b.txt", b"");
        d2.write("only_d2.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

        assert_eq!(index.conflicts[0].overridden_by.len(), 2);
        assert_eq!(index.conflicts[1].overrides.len(), 2);
        assert!(
            !index.conflicts[0]
                .overridden_by
                .contains(&PathBuf::from("only_d1.txt"))
        );
        assert!(
            !index.conflicts[1]
                .overrides
                .contains(&PathBuf::from("only_d2.txt"))
        );
    }

    #[test]
    fn from_file_lists_produces_same_result_as_from_directories() {
        let d1 = TempDir::new("ci_ffl_d1");
        let d2 = TempDir::new("ci_ffl_d2");
        d1.write("shared.txt", b"");
        d1.write("only_d1.txt", b"");
        d2.write("shared.txt", b"");
        d2.write("only_d2.txt", b"");

        let from_dirs = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

        // Build the same index manually via from_file_lists
        let lists = vec![
            (
                d1.path().to_path_buf(),
                vec![PathBuf::from("shared.txt"), PathBuf::from("only_d1.txt")],
            ),
            (
                d2.path().to_path_buf(),
                vec![PathBuf::from("shared.txt"), PathBuf::from("only_d2.txt")],
            ),
        ];
        let from_lists = ConflictIndex::from_file_lists(lists);

        assert_eq!(
            from_dirs.conflicts[0].overrides,
            from_lists.conflicts[0].overrides
        );
        assert_eq!(
            from_dirs.conflicts[0].overridden_by,
            from_lists.conflicts[0].overridden_by
        );
        assert_eq!(
            from_dirs.conflicts[1].overrides,
            from_lists.conflicts[1].overrides
        );
        assert_eq!(
            from_dirs.conflicts[1].overridden_by,
            from_lists.conflicts[1].overridden_by
        );
    }

    #[test]
    fn from_file_lists_archive_before_dir_gives_archive_lower_priority() {
        // Simulate: archive provides "textures/foo.dds", dir overrides it.
        // Archive is index 0 (lower priority), dir is index 1 (higher priority).
        let lists = vec![
            (
                PathBuf::from("Morrowind.bsa"),
                vec![PathBuf::from("textures/foo.dds")],
            ),
            (
                PathBuf::from("/data/mod"),
                vec![PathBuf::from("textures/foo.dds")],
            ),
        ];
        let index = ConflictIndex::from_file_lists(lists);

        // archive (0): overridden by the dir, overrides nothing
        assert!(
            !index.conflicts[0].has_overrides(),
            "archive should not override anything"
        );
        assert!(
            index.conflicts[0].is_overridden(),
            "archive should be overridden by the dir"
        );

        // dir (1): overrides the archive, not overridden by anything
        assert!(
            index.conflicts[1].has_overrides(),
            "dir should override the archive"
        );
        assert!(
            !index.conflicts[1].is_overridden(),
            "nothing overrides the dir"
        );
    }

    // ---- conflicts_report ----

    #[test]
    fn conflicts_report_relative_paths() {
        let d1 = TempDir::new("ci_report_cr_d1");
        let d2 = TempDir::new("ci_report_cr_d2");
        d1.write("shared.txt", b"");
        d2.write("shared.txt", b"");
        d1.write("only1.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.conflicts_report(true);

        assert_eq!(report.sources.len(), 2);
        // d2 (idx 1) overrides shared.txt — its overrides list should contain the relative key
        let d2_entry = &report.sources[1];
        assert!(!d2_entry.overrides.is_empty(), "d2 should have overrides");
        assert!(
            d2_entry
                .overrides
                .iter()
                .any(|p| p == &PathBuf::from("shared.txt")),
            "overrides should contain the relative key 'shared.txt'"
        );
    }

    #[test]
    fn conflicts_report_sorted() {
        let d1 = TempDir::new("ci_report_cr_sort_d1");
        let d2 = TempDir::new("ci_report_cr_sort_d2");
        d1.write("b.txt", b"");
        d1.write("a.txt", b"");
        d2.write("b.txt", b"");
        d2.write("a.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.conflicts_report(true);

        let overrides = &report.sources[1].overrides;
        assert!(
            overrides.windows(2).all(|w| w[0] <= w[1]),
            "overrides should be sorted"
        );
        let overridden_by = &report.sources[0].overridden_by;
        assert!(
            overridden_by.windows(2).all(|w| w[0] <= w[1]),
            "overridden_by should be sorted"
        );
    }

    #[test]
    fn conflicts_report_formats_archive_sources_as_entries() {
        let index = ConflictIndex::from_file_lists(vec![
            (
                PathBuf::from("/data/archive.zip"),
                vec![PathBuf::from("textures/foo.dds")],
            ),
            (
                PathBuf::from("/data/mod"),
                vec![PathBuf::from("textures/foo.dds")],
            ),
        ]);

        let report = index.conflicts_report(false);
        assert_eq!(
            report.sources[0].overridden_by,
            vec![PathBuf::from("/data/archive.zip::textures/foo.dds")]
        );
    }

    // ---- shadowed_report ----

    #[test]
    fn shadowed_report_clean_source_excluded() {
        let d1 = TempDir::new("ci_report_sr_clean_d1");
        let d2 = TempDir::new("ci_report_sr_clean_d2");
        // No shared files — no conflicts
        d1.write("only1.txt", b"");
        d2.write("only2.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.shadowed_report(true);
        assert!(
            report.sources.is_empty(),
            "no sources should appear when there are no conflicts"
        );
    }

    #[test]
    fn shadowed_report_overridden_source_included_sorted() {
        let d1 = TempDir::new("ci_report_sr_over_d1");
        let d2 = TempDir::new("ci_report_sr_over_d2");
        d1.write("b.txt", b"");
        d1.write("a.txt", b"");
        d2.write("b.txt", b"");
        d2.write("a.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.shadowed_report(true);

        // d1 is overridden by d2, so it should appear in the shadowed report
        assert!(!report.sources.is_empty(), "d1 should appear as shadowed");
        let shadowed = &report.sources[0];
        assert!(
            shadowed.shadowed_files.windows(2).all(|w| w[0] <= w[1]),
            "shadowed_files should be sorted"
        );
    }

    #[test]
    fn shadowed_report_excludes_partially_overridden_source() {
        let d1 = TempDir::new("ci_report_sr_partial_d1");
        let d2 = TempDir::new("ci_report_sr_partial_d2");
        d1.write("shared.txt", b"");
        d1.write("unique.txt", b"");
        d2.write("shared.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.shadowed_report(true);

        assert!(report.sources.is_empty());
    }

    // ---- which ----

    #[test]
    fn which_returns_none_for_missing_path() {
        let d1 = TempDir::new("ci_report_wh_none_d1");
        d1.write("present.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path()]);
        let (vfs, _) = VFS::from_directories_with_conflict_index(vec![d1.path()], None);
        let result = index.which(&vfs, Path::new("absent.txt"));
        assert!(result.is_none());
    }

    #[test]
    fn which_unique_file_is_unique() {
        let d1 = TempDir::new("ci_report_wh_uniq_d1");
        d1.write("unique.txt", b"x");

        let index = ConflictIndex::from_directories(vec![d1.path()]);
        let (vfs, _) = VFS::from_directories_with_conflict_index(vec![d1.path()], None);
        let result = index
            .which(&vfs, Path::new("unique.txt"))
            .expect("file should be found");
        assert!(result.is_unique, "file only in one source should be unique");
        assert!(result.also_in.is_empty());
    }

    #[test]
    fn which_shared_file_has_also_in() {
        let d1 = TempDir::new("ci_report_wh_shared_d1");
        let d2 = TempDir::new("ci_report_wh_shared_d2");
        d1.write("shared.txt", b"d1");
        d2.write("shared.txt", b"d2");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let (vfs, _) = VFS::from_directories_with_conflict_index(vec![d1.path(), d2.path()], None);
        let result = index
            .which(&vfs, Path::new("shared.txt"))
            .expect("file should be found");
        // d2 wins (higher priority), so also_in should contain d1
        assert!(!result.is_unique);
        assert!(
            result.also_in.contains(&d1.path().to_path_buf()),
            "lower-priority dir should be in also_in"
        );
    }

    // ---- stats ----

    #[test]
    fn stats_wins_and_overrides() {
        let d1 = TempDir::new("ci_report_st_d1");
        let d2 = TempDir::new("ci_report_st_d2");
        d1.write("shared.txt", b"d1");
        d1.write("only1.txt", b"d1");
        d2.write("shared.txt", b"d2");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let (vfs, _) = VFS::from_directories_with_conflict_index(vec![d1.path(), d2.path()], None);
        let report = index.stats(&vfs);

        assert_eq!(report.rows.len(), 2);
        let row_d1 = &report.rows[0]; // d1 = lower priority
        let row_d2 = &report.rows[1]; // d2 = higher priority

        // d2 serves shared.txt — it wins on that file
        assert!(row_d2.wins > 0, "d2 should have wins > 0");
        // d2 overrides d1 on shared.txt
        assert_eq!(row_d2.overrides, 1, "d2 overrides one file");
        // d1 is overridden on shared.txt
        assert!(row_d1.overridden > 0, "d1 should have overridden > 0");
    }

    #[test]
    fn stats_attributes_nested_loose_files_to_most_specific_source() {
        let parent = TempDir::new("ci_report_st_nested_parent");
        let child = TempDir(parent.path().join("child"));
        child.write("foo.txt", b"child");

        let index = ConflictIndex::from_directories(vec![parent.path(), child.path()]);
        let (vfs, _) =
            VFS::from_directories_with_conflict_index(vec![parent.path(), child.path()], None);
        let report = index.stats(&vfs);

        assert_eq!(report.rows[0].wins, 1, "parent should win child/foo.txt");
        assert_eq!(report.rows[1].wins, 1, "child should win foo.txt");
    }

    // ---- diff_report ----

    #[test]
    fn diff_report_shared_and_unique() {
        let d1 = TempDir::new("ci_report_dr_d1");
        let d2 = TempDir::new("ci_report_dr_d2");
        d1.write("shared.txt", b"");
        d1.write("only_a.txt", b"");
        d2.write("shared.txt", b"");
        d2.write("only_b.txt", b"");

        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.diff_report(d1.path(), d2.path());

        assert!(
            report.shared.contains(&PathBuf::from("shared.txt")),
            "shared.txt should be in shared"
        );
        assert!(
            report.only_in_a.contains(&PathBuf::from("only_a.txt")),
            "only_a.txt should be only in a"
        );
        assert!(
            report.only_in_b.contains(&PathBuf::from("only_b.txt")),
            "only_b.txt should be only in b"
        );
    }

    #[test]
    fn diff_report_higher_priority_is_later_dir() {
        let d1 = TempDir::new("ci_report_dr_prio_d1");
        let d2 = TempDir::new("ci_report_dr_prio_d2");
        d1.write("f.txt", b"");
        d2.write("f.txt", b"");

        // d2 comes later, so it should have higher priority
        let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
        let report = index.diff_report(d1.path(), d2.path());

        assert_eq!(
            report.higher_priority,
            d2.path().to_path_buf(),
            "d2 (later) should have higher priority"
        );
    }
}
