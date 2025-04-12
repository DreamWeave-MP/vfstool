pub mod directory_node;
pub mod vfs;
pub mod vfs_file;

pub(crate) use directory_node::DirectoryNode;
pub use vfs::VFS;
pub use vfs_file::VfsFile;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub type DisplayTree = BTreeMap<PathBuf, DirectoryNode>;

pub enum SerializeType {
    Json,
    Yaml,
    Toml,
}

pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let normalized = path
        .as_ref()
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .map(|&byte| match byte {
            b'\\' => '/' as u8,
            b'A'..=b'Z' => byte + 32,
            _ => byte,
        })
        .collect::<Vec<_>>();

    PathBuf::from(unsafe { std::ffi::OsString::from_encoded_bytes_unchecked(normalized) })
}

pub mod archives {
    use super::VfsFile;
    use ba2::{self, prelude::*, tes3::Archive as TES3Archive};
    use std::{
        collections::HashMap,
        fs::File,
        path::{Path, PathBuf},
        sync::Arc,
    };

    #[derive(Debug)]
    pub enum TypedArchive {
        Tes3(ba2::tes3::Archive<'static>),
        Tes4(ba2::tes4::Archive<'static>),
        Fo4(ba2::fo4::Archive<'static>),
    }

    /// Privatize the shit out of this
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

    pub fn from_set(file_map: &HashMap<PathBuf, VfsFile>, archive_list: Vec<&str>) -> ArchiveList {
        archive_list
            .into_iter()
            .filter_map(|archive| {
                let archive_path = PathBuf::from(archive.to_ascii_lowercase());
                // Try to get the archive from the file map
                file_map.get(&archive_path).and_then(|valid_archive| {
                    let path = valid_archive.path();
                    // Attempt to open the archive file
                    File::open(&path).ok().and_then(|mut file_handle| {
                        // Attempt to read the archive
                        match ba2::guess_format(&mut file_handle) {
                            None => None,
                            Some(format) => match format {
                                ba2::FileFormat::TES3 => {
                                    TES3Archive::read(&file_handle).ok().map(|archive| {
                                        Arc::new(StoredArchive {
                                            file_handle,
                                            archive: TypedArchive::Tes3(archive),
                                            path: path.to_path_buf(),
                                        })
                                    })
                                }
                                ba2::FileFormat::TES4 => ba2::tes4::Archive::read(&file_handle)
                                    .ok()
                                    .map(|(archive, _meta)| {
                                        Arc::new(StoredArchive {
                                            file_handle,
                                            archive: TypedArchive::Tes4(archive),
                                            path: path.to_path_buf(),
                                        })
                                    }),
                                ba2::FileFormat::FO4 => ba2::fo4::Archive::read(&file_handle)
                                    .ok()
                                    .map(|(archive, _meta)| {
                                        Arc::new(StoredArchive {
                                            file_handle,
                                            archive: TypedArchive::Fo4(archive),
                                            path: path.to_path_buf(),
                                        })
                                    }),
                            },
                        }
                    })
                })
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
                            let normalized = crate::normalize_path(&name_string);
                            (
                                normalized,
                                VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                            )
                        })),
                        TypedArchive::Tes4(data) => Box::new(data.iter().map(|(key, _value)| {
                            let name_string = key.name().to_string();
                            let normalized = crate::normalize_path(&name_string);
                            (
                                normalized,
                                VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                            )
                        })),
                        TypedArchive::Fo4(data) => Box::new(data.iter().map(|(key, _value)| {
                            let name_string = key.name().to_string();
                            let normalized = crate::normalize_path(&name_string);
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
