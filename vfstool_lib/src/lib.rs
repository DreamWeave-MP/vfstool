// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(missing_docs)]
//! Virtual file system library for OpenMW modding tools.
/// Conflict analysis: per-source override and overridden-by sets.
pub mod conflict;
/// Higher-level analysis APIs: provenance, semantic conflicts, lock manifests, and simulations.
pub mod analysis;
/// Tree node used for display and serialization of VFS directory structure.
pub mod directory_node;
/// Declarative policy rules and evaluation against VFS/layer state.
pub mod policy;
/// Report types returned by conflict, shadowed, which, stats, and diff subcommands.
pub mod reports;
/// Utilities for the MO2-style `run` workflow: dump, snapshot, and finalize.
pub mod run;
/// Core [`VFS`] struct and directory-construction logic.
pub mod vfs;
/// [`VfsFile`] wrapper for loose and archive-backed files.
pub mod vfs_file;

pub(crate) use directory_node::DirectoryNode;
pub use analysis::{
    ArchiveHashMode, BucketDelta, DriftEntry, DriftKind, DriftReport, LayerIndex, ProviderRecord,
    ProvenanceChain, ReorderOp, SemanticConflict, SemanticConflictReport, SemanticOpts,
    SemanticProvider, SemanticRelation, SimOpts, SimulationDelta, SourceDelta, SourceKind,
    SourceMeta, VfsLock, VfsLockEntry,
};
pub use conflict::{ConflictIndex, SourceConflicts};
pub use policy::{Policy, PolicyResult, Rule, Severity, Violation};
pub use reports::{
    CollapseOptions, ConflictSourceEntry, ConflictsReport, DiffReport, ShadowedReport,
    ShadowedSource, StatsReport, StatsRow, WhichResult,
};
pub use run::{changed_files, run_finalize, run_setup, snapshot_directory, Snapshot};
pub use vfs::{DirectoryDiff, VFS};
pub use vfs_file::VfsFile;

use std::{
    borrow::Cow,
    collections::BTreeMap,
    ffi::OsString,
    mem,
    path::{Path, PathBuf},
};

/// Sorted map from a directory name to its [`DirectoryNode`], used for display and serialization.
pub type DisplayTree = BTreeMap<PathBuf, DirectoryNode>;

/// Output format for [`serialize_value`] and [`VFS::serialize_from_tree`].
pub enum SerializeType {
    /// Serialize as JSON.
    Json,
    /// Serialize as YAML.
    Yaml,
    /// Serialize as TOML.
    Toml,
}

/// Serialize any `serde::Serialize` value to JSON, YAML, or TOML.
#[cfg(feature = "serialize")]
pub fn serialize_value<T: serde::Serialize>(
    value: &T,
    write_type: SerializeType,
) -> std::io::Result<String> {
    fn to_io_error<E: std::fmt::Display>(err: E) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
    }
    match write_type {
        SerializeType::Json => serde_json::to_string(value).map_err(to_io_error),
        SerializeType::Yaml => serde_yaml::to_string(value).map_err(to_io_error),
        SerializeType::Toml => toml::to_string_pretty(value).map_err(to_io_error),
    }
}

/// Normalize a path by converting backslashes to forward slashes and lowercasing ASCII letters.
///
/// Returns a borrowed `Cow` when no transformation is needed, avoiding allocation on the fast path.
pub fn normalize_path<P: AsRef<Path> + ?Sized>(path: &P) -> Cow<'_, Path> {
    let p = path.as_ref();
    let bytes = p.as_os_str().as_encoded_bytes();
    if !bytes.iter().any(|&b| b == b'\\' || b.is_ascii_uppercase()) {
        return Cow::Borrowed(p);
    }
    let normalized: Vec<u8> = bytes
        .iter()
        .map(|&byte| match byte {
            b'\\' => b'/',
            b'A'..=b'Z' => byte + 32,
            _ => byte,
        })
        .collect();
    Cow::Owned(PathBuf::from(unsafe {
        OsString::from_encoded_bytes_unchecked(normalized)
    }))
}

/// Normalizes a [`PathBuf`] in-place, reusing its heap allocation.
///
/// Converts backslashes to forward slashes and lowercases ASCII letters.
/// No-op if the path requires no changes.
pub fn normalize_path_in_place(path: &mut PathBuf) {
    if !path
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&b| b == b'\\' || b.is_ascii_uppercase())
    {
        return;
    }
    let mut bytes = mem::take(path).into_os_string().into_encoded_bytes();
    for byte in bytes.iter_mut() {
        match *byte {
            b'\\' => *byte = b'/',
            b'A'..=b'Z' => *byte += 32,
            _ => {}
        }
    }
    // SAFETY: We only modified ASCII bytes (\ → / and A–Z → a–z), which
    // preserves the encoding invariant on all platforms.
    *path = PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(bytes) });
}

/// Returns `true` if the path has a ZIP or PK3 extension (case-insensitive).
#[cfg(feature = "zip")]
pub(crate) fn is_zip_or_pk3(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("pk3"))
}

/// Low-level archive loading and enumeration (BSA, BA2, ZIP, PK3).
///
/// Enabled when the `bsa` or `zip` feature is active.
#[cfg(any(feature = "bsa", feature = "zip"))]
pub mod archives {
    use ahash::AHashMap;
    use std::{
        fmt,
        fs::File,
        path::{Path, PathBuf},
        sync::Arc,
    };

    #[cfg(feature = "zip")]
    use std::sync::Mutex;

    use super::VfsFile;

    #[cfg(feature = "bsa")]
    use ba2::{self, prelude::*, tes3::Archive as TES3Archive};

    /// An open archive, tagged by its format.
    pub enum TypedArchive {
        /// TES3 (Morrowind) BSA archive.
        #[cfg(feature = "bsa")]
        Tes3(ba2::tes3::Archive<'static>),
        /// TES4 (Oblivion/Skyrim) BSA archive.
        #[cfg(feature = "bsa")]
        Tes4(ba2::tes4::Archive<'static>),
        /// Fallout 4 BA2 archive.
        #[cfg(feature = "bsa")]
        Fo4(ba2::fo4::Archive<'static>),
        /// ZIP or PK3 archive.
        #[cfg(feature = "zip")]
        Zip(Mutex<zip::ZipArchive<File>>),
    }

    impl fmt::Debug for TypedArchive {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                #[cfg(feature = "bsa")]
                Self::Tes3(_) => f.write_str("TypedArchive::Tes3"),
                #[cfg(feature = "bsa")]
                Self::Tes4(_) => f.write_str("TypedArchive::Tes4"),
                #[cfg(feature = "bsa")]
                Self::Fo4(_) => f.write_str("TypedArchive::Fo4"),
                #[cfg(feature = "zip")]
                Self::Zip(_) => f.write_str("TypedArchive::Zip"),
            }
        }
    }

    /// An opened, heap-allocated archive together with its on-disk path.
    #[derive(Debug)]
    pub struct StoredArchive {
        /// Keeps the BSA/BA2 memory-map file handle alive.
        /// `None` for ZIP/PK3 archives — `ZipArchive` owns its file handle internally.
        #[allow(dead_code)]
        file_handle: Option<File>,
        archive: TypedArchive,
        path: PathBuf,
    }

    impl StoredArchive {
        /// Returns the typed archive handle.
        pub fn handle(&self) -> &TypedArchive {
            &self.archive
        }

        /// Returns the absolute path to the archive file on disk.
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    /// Ordered list of open archive handles.
    pub type ArchiveList = Vec<Arc<StoredArchive>>;

    /// Open every archive named in `archive_list` that can be resolved through `file_map`.
    pub fn from_set(file_map: &AHashMap<PathBuf, VfsFile>, archive_list: &[&str]) -> ArchiveList {
        archive_list
            .iter()
            .copied()
            .filter_map(|archive| {
                let archive_path = PathBuf::from(archive.to_ascii_lowercase());

                let valid_archive = match file_map.get(&archive_path) {
                    Some(f) => f,
                    None => {
                        eprintln!("vfstool: warning: archive '{archive}' not found in any data directory, skipping");
                        return None;
                    }
                };

                open_archive(valid_archive.path())
            })
            .collect()
    }

    /// Try to open a single archive file, detecting its format by extension and content.
    ///
    /// ZIP/PK3 files are identified by extension; BSA/BA2 files are identified by
    /// magic bytes. Returns `None` with a warning on any failure.
    #[allow(unreachable_code)]
    fn open_archive(path: &Path) -> Option<Arc<StoredArchive>> {
        // ZIP / PK3 — detect by extension before touching file content.
        #[cfg(feature = "zip")]
        if crate::is_zip_or_pk3(path) {
            let file = match File::open(path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("vfstool: warning: failed to open zip '{}': {e}", path.display());
                    return None;
                }
            };
            return match zip::ZipArchive::new(file) {
                Ok(archive) => Some(Arc::new(StoredArchive {
                    file_handle: None,
                    archive: TypedArchive::Zip(Mutex::new(archive)),
                    path: path.to_path_buf(),
                })),
                Err(e) => {
                    eprintln!("vfstool: warning: failed to read zip '{}': {e}", path.display());
                    None
                }
            };
        }

        // BSA / BA2 — detect by magic bytes.
        #[cfg(feature = "bsa")]
        {
            let mut file_handle = match File::open(path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("vfstool: warning: failed to open archive '{}': {e}", path.display());
                    return None;
                }
            };
            let format = match ba2::guess_format(&mut file_handle) {
                Some(f) => f,
                None => {
                    eprintln!("vfstool: warning: could not determine format of archive '{}', skipping", path.display());
                    return None;
                }
            };
            return match format {
                ba2::FileFormat::TES3 => match TES3Archive::read(&file_handle) {
                    Ok(archive) => Some(Arc::new(StoredArchive {
                        file_handle: Some(file_handle),
                        archive: TypedArchive::Tes3(archive),
                        path: path.to_path_buf(),
                    })),
                    Err(e) => {
                        eprintln!("vfstool: warning: failed to read TES3 archive '{}': {e}", path.display());
                        None
                    }
                },
                ba2::FileFormat::TES4 => match ba2::tes4::Archive::read(&file_handle) {
                    Ok((archive, _meta)) => Some(Arc::new(StoredArchive {
                        file_handle: Some(file_handle),
                        archive: TypedArchive::Tes4(archive),
                        path: path.to_path_buf(),
                    })),
                    Err(e) => {
                        eprintln!("vfstool: warning: failed to read TES4 archive '{}': {e}", path.display());
                        None
                    }
                },
                ba2::FileFormat::FO4 => match ba2::fo4::Archive::read(&file_handle) {
                    Ok((archive, _meta)) => Some(Arc::new(StoredArchive {
                        file_handle: Some(file_handle),
                        archive: TypedArchive::Fo4(archive),
                        path: path.to_path_buf(),
                    })),
                    Err(e) => {
                        eprintln!("vfstool: warning: failed to read FO4 archive '{}': {e}", path.display());
                        None
                    }
                },
            };
        }

        eprintln!(
            "vfstool: warning: '{}' is not a recognized archive format, skipping",
            path.display()
        );
        None
    }

    /// Return the normalized VFS paths for all files in an already-open archive.
    ///
    /// Used by [`VFS::from_directories_with_conflict_index`] to enumerate archive
    /// contents without re-opening the archive from disk.
    pub fn archive_paths(stored: &StoredArchive) -> Vec<PathBuf> {
        match &stored.archive {
            #[cfg(feature = "bsa")]
            TypedArchive::Tes3(data) => data
                .iter()
                .map(|(key, _)| {
                    let mut p = PathBuf::from(key.name().to_string());
                    crate::normalize_path_in_place(&mut p);
                    p
                })
                .collect(),
            #[cfg(feature = "bsa")]
            TypedArchive::Tes4(data) => data
                .iter()
                .flat_map(|(dir_key, dir)| {
                    let dir_str = dir_key.name().to_string();
                    dir.iter()
                        .map(move |(key, _)| {
                            let mut p = PathBuf::from(format!("{}\\{}", dir_str, key.name()));
                            crate::normalize_path_in_place(&mut p);
                            p
                        })
                        .collect::<Vec<_>>()
                })
                .collect(),
            #[cfg(feature = "bsa")]
            TypedArchive::Fo4(data) => data
                .iter()
                .map(|(key, _)| {
                    let mut p = PathBuf::from(key.name().to_string());
                    crate::normalize_path_in_place(&mut p);
                    p
                })
                .collect(),
            #[cfg(feature = "zip")]
            TypedArchive::Zip(archive) => {
                let guard = archive.lock().expect("zip mutex should not be poisoned");
                guard
                    .file_names()
                    .filter(|name| !name.ends_with('/'))
                    .map(|name| {
                        let mut p = PathBuf::from(name);
                        crate::normalize_path_in_place(&mut p);
                        p
                    })
                    .collect()
            }
        }
    }

    /// Build a normalized-path → [`VfsFile`] map from an [`ArchiveList`].
    pub fn file_map(archives: ArchiveList) -> AHashMap<PathBuf, VfsFile> {
        archives
            .iter()
            .flat_map(|stored_archive| {
                let iter: Box<dyn Iterator<Item = (PathBuf, VfsFile)>> =
                    match &stored_archive.archive {
                        #[cfg(feature = "bsa")]
                        TypedArchive::Tes3(data) => Box::new(data.iter().map(|(key, _value)| {
                            let name_string = key.name().to_string();
                            let mut normalized = PathBuf::from(&name_string);
                            crate::normalize_path_in_place(&mut normalized);
                            (
                                normalized,
                                VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                            )
                        })),
                        #[cfg(feature = "bsa")]
                        TypedArchive::Tes4(data) => {
                            Box::new(data.iter().flat_map(move |(dir_key, dir)| {
                                let dir_string = dir_key.name();
                                dir.iter().map(move |(key, _value)| {
                                    let archive_path = format!("{}\\{}", dir_string, key.name());
                                    let mut normalized = PathBuf::from(&archive_path);
                                    crate::normalize_path_in_place(&mut normalized);
                                    let vfs_file = VfsFile::from_archive(
                                        normalized.to_string_lossy(),
                                        Arc::clone(stored_archive),
                                    );
                                    (normalized, vfs_file)
                                })
                            }))
                        }
                        #[cfg(feature = "bsa")]
                        TypedArchive::Fo4(data) => Box::new(data.iter().map(|(key, _value)| {
                            let name_string = key.name().to_string();
                            let mut normalized = PathBuf::from(&name_string);
                            crate::normalize_path_in_place(&mut normalized);
                            (
                                normalized,
                                VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                            )
                        })),
                        #[cfg(feature = "zip")]
                        TypedArchive::Zip(archive) => {
                            // Collect eagerly — the iterator borrows from the MutexGuard
                            // which is local and cannot escape the match arm.
                            let guard =
                                archive.lock().expect("zip mutex should not be poisoned");
                            let entries: Vec<(PathBuf, VfsFile)> = guard
                                .file_names()
                                .filter(|name| !name.ends_with('/'))
                                .map(|name| {
                                    // Keep the original name for lookup in open(); normalize
                                    // separately for the VFS HashMap key.
                                    let original_name = name.to_string();
                                    let mut normalized = PathBuf::from(name);
                                    crate::normalize_path_in_place(&mut normalized);
                                    let vfs_file = VfsFile::from_archive(
                                        &original_name,
                                        Arc::clone(stored_archive),
                                    );
                                    (normalized, vfs_file)
                                })
                                .collect();
                            Box::new(entries.into_iter())
                        }
                    };
                iter
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // --- normalize_path ---

    #[test]
    fn normalize_already_normalized_is_noop() {
        let p = "textures/landscape/foo.dds";
        assert_eq!(normalize_path(p), PathBuf::from(p));
    }

    #[test]
    fn normalize_backslash_to_forward_slash() {
        assert_eq!(
            normalize_path("textures\\landscape\\foo.dds"),
            PathBuf::from("textures/landscape/foo.dds"),
        );
    }

    #[test]
    fn normalize_uppercase_to_lowercase() {
        assert_eq!(
            normalize_path("Meshes/Actors/Foo.NIF"),
            PathBuf::from("meshes/actors/foo.nif"),
        );
    }

    #[test]
    fn normalize_windows_path_combined() {
        assert_eq!(
            normalize_path("Meshes\\Actors\\XBase_Anim.NIF"),
            PathBuf::from("meshes/actors/xbase_anim.nif"),
        );
    }

    #[test]
    fn normalize_path_with_spaces_preserved() {
        assert_eq!(
            normalize_path("Data Files\\Morrowind.esm"),
            PathBuf::from("data files/morrowind.esm"),
        );
    }

    #[test]
    fn normalize_empty_path() {
        assert_eq!(normalize_path(""), PathBuf::from(""));
    }

    #[test]
    fn normalize_single_component_uppercase() {
        assert_eq!(normalize_path("Morrowind.ESM"), PathBuf::from("morrowind.esm"));
    }

    #[test]
    fn normalize_already_lowercase_forward_slash_fast_path() {
        // Fast-path kicks in — result equals input, no transform needed
        let p = "data files/tribunal.esm";
        assert_eq!(normalize_path(p), PathBuf::from(p));
    }

    #[test]
    fn normalize_non_ascii_passthrough() {
        // Non-ASCII bytes pass through unchanged; only ASCII letters and backslashes transform
        let input = "Textures/Nordström.dds";
        let result = normalize_path(input).to_string_lossy().into_owned();
        assert!(result.starts_with("textures/"), "ASCII prefix should be lowercased");
        assert!(result.contains("tröm"), "non-ASCII content should be preserved unchanged");
    }

    // --- normalize_path_in_place ---

    #[test]
    fn normalize_in_place_noop_when_already_normalized() {
        let original = PathBuf::from("textures/landscape/foo.dds");
        let mut path = original.clone();
        normalize_path_in_place(&mut path);
        assert_eq!(path, original);
    }

    #[test]
    fn normalize_in_place_backslash() {
        let mut path = PathBuf::from("textures\\landscape\\foo.dds");
        normalize_path_in_place(&mut path);
        assert_eq!(path, PathBuf::from("textures/landscape/foo.dds"));
    }

    #[test]
    fn normalize_in_place_uppercase() {
        let mut path = PathBuf::from("Meshes/Actors/Foo.NIF");
        normalize_path_in_place(&mut path);
        assert_eq!(path, PathBuf::from("meshes/actors/foo.nif"));
    }

    #[test]
    fn normalize_in_place_empty_path() {
        let mut path = PathBuf::from("");
        normalize_path_in_place(&mut path);
        assert_eq!(path, PathBuf::from(""));
    }

    #[test]
    fn normalize_in_place_matches_allocating_version() {
        // Property test: both functions must agree on every input
        let cases: &[&str] = &[
            "Meshes\\Actors\\XBase_Anim.NIF",
            "TEXTURES/LANDSCAPE/foo.dds",
            "already/normalized/path",
            "",
            "Morrowind.ESM",
            "mixed\\Case/Path\\FILE.ext",
            "Data Files\\Tribunal.esm",
            "textures/landscape/foo.dds",
        ];
        for &case in cases {
            let mut in_place = PathBuf::from(case);
            normalize_path_in_place(&mut in_place);
            assert_eq!(
                in_place,
                normalize_path(case),
                "in_place and allocating versions disagree for input {case:?}",
            );
        }
    }
}
