pub mod directory_node;
pub mod vfs;
pub mod vfs_file;

pub(crate) use directory_node::DirectoryNode;
pub use vfs::{DirectoryDiff, VFS};
pub use vfs_file::VfsFile;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    mem,
    path::{Path, PathBuf},
};

pub type DisplayTree = BTreeMap<PathBuf, DirectoryNode>;

pub enum SerializeType {
    Json,
    Yaml,
    Toml,
}

pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let bytes = path.as_ref().as_os_str().as_encoded_bytes();
    if !bytes.iter().any(|&b| b == b'\\' || b.is_ascii_uppercase()) {
        return path.as_ref().to_path_buf();
    }
    let normalized: Vec<u8> = bytes
        .iter()
        .map(|&byte| match byte {
            b'\\' => b'/',
            b'A'..=b'Z' => byte + 32,
            _ => byte,
        })
        .collect();
    PathBuf::from(unsafe { OsString::from_encoded_bytes_unchecked(normalized) })
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

#[cfg(feature = "bsa")]
pub mod archives {
    use std::{
        collections::HashMap,
        fs::File,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use super::VfsFile;
    use ba2::{self, prelude::*, tes3::Archive as TES3Archive};

    #[derive(Debug)]
    pub enum TypedArchive {
        Tes3(ba2::tes3::Archive<'static>),
        Tes4(ba2::tes4::Archive<'static>),
        Fo4(ba2::fo4::Archive<'static>),
    }

    #[derive(Debug)]
    pub struct StoredArchive {
        // Not actually used, but necessary to keep the `archive` alive
        #[allow(dead_code)]
        file_handle: File,
        archive: TypedArchive,
        path: PathBuf,
    }

    impl StoredArchive {
        pub fn handle(&self) -> &TypedArchive {
            &self.archive
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    pub type ArchiveList = Vec<Arc<StoredArchive>>;

    pub fn from_set(file_map: &HashMap<PathBuf, VfsFile>, archive_list: &[&str]) -> ArchiveList {
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

                let path = valid_archive.path();

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

                match format {
                    ba2::FileFormat::TES3 => match TES3Archive::read(&file_handle) {
                        Ok(archive) => Some(Arc::new(StoredArchive {
                            file_handle,
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
                            file_handle,
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
                            file_handle,
                            archive: TypedArchive::Fo4(archive),
                            path: path.to_path_buf(),
                        })),
                        Err(e) => {
                            eprintln!("vfstool: warning: failed to read FO4 archive '{}': {e}", path.display());
                            None
                        }
                    },
                }
            })
            .collect()
    }

    pub fn file_map(archives: ArchiveList) -> HashMap<PathBuf, VfsFile> {
        archives
            .iter()
            .flat_map(|stored_archive| {
                let iter: Box<dyn Iterator<Item = (PathBuf, VfsFile)>> =
                    match &stored_archive.archive {
                        TypedArchive::Tes3(data) => Box::new(data.iter().map(|(key, _value)| {
                            let name_string = key.name().to_string();
                            let mut normalized = PathBuf::from(&name_string);
                            crate::normalize_path_in_place(&mut normalized);
                            (
                                normalized,
                                VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                            )
                        })),
                        TypedArchive::Tes4(data) => {
                            Box::new(data.iter().flat_map(move |(dir_key, dir)| {
                                let dir_string = dir_key.name();
                                dir.iter().map(move |(key, _value)| {
                                    let archive_path = format!("{}\\{}", dir_string, key.name());
                                    let mut normalized = PathBuf::from(&archive_path);
                                    crate::normalize_path_in_place(&mut normalized);
                                    let vfs_file = VfsFile::from_archive(
                                        &normalized.to_string_lossy(),
                                        Arc::clone(stored_archive),
                                    );
                                    (normalized, vfs_file)
                                })
                            }))
                        }
                        TypedArchive::Fo4(data) => Box::new(data.iter().map(|(key, _value)| {
                            let name_string = key.name().to_string();
                            let mut normalized = PathBuf::from(&name_string);
                            crate::normalize_path_in_place(&mut normalized);
                            (
                                normalized,
                                VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                            )
                        })),
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
