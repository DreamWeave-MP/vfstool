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

/// Lowercase path and convert path separators to unix-style
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    PathBuf::from(
        path.as_ref()
            .to_string_lossy()
            .to_lowercase()
            .replace("\\", "/"), // Additional handling for duplicate path separators
                                 // But probably too expensive to actually use?
                                 // .split('/')
                                 // .filter(|s| !s.is_empty())
                                 // .collect::<Vec<_>>()
                                 // .join("/"),
    )
}

pub mod archives {
    use super::VfsFile;
    use ba2::{prelude::*, tes3::Archive as TES3Archive};
    use std::{collections::HashMap, fs::File, path::PathBuf, sync::Arc};

    /// Privatize the shit out of this
    #[derive(Debug)]
    pub struct StoredArchive {
        // Not actually used, but necessary to keep the `archive` alive
        #[allow(dead_code)]
        file_handle: File,
        pub archive: TES3Archive<'static>,
        pub path: PathBuf,
    }

    pub type ArchiveList = Vec<Arc<StoredArchive>>;

    pub fn in_config(config: &openmw_cfg::Ini) -> Vec<&str> {
        config
            .general_section()
            .iter()
            .filter_map(|(k, v)| match k == "fallback-archive" {
                false => None,
                true => Some(v),
            })
            .collect()
    }

    pub fn from_set(file_map: &HashMap<PathBuf, VfsFile>, archive_list: Vec<&str>) -> ArchiveList {
        archive_list
            .into_iter()
            .filter_map(|archive| {
                let archive_path = PathBuf::from(archive.to_ascii_lowercase());
                // Try to get the archive from the file map
                file_map.get(&archive_path).and_then(|valid_archive| {
                    let path = valid_archive.path();
                    // Attempt to open the archive file
                    File::open(&path).ok().and_then(|file_handle| {
                        // Attempt to read the archive
                        TES3Archive::read(&file_handle).ok().map(|archive| {
                            Arc::new(StoredArchive {
                                file_handle,
                                archive,
                                path: path.to_path_buf(),
                            })
                        })
                    })
                })
            })
            .collect()
    }

    pub fn file_map(archives: ArchiveList) -> HashMap<PathBuf, VfsFile> {
        archives
            .iter()
            .flat_map(|stored_archive| {
                stored_archive.archive.iter().map(move |(key, _value)| {
                    let name_string = key.name().to_string();
                    let normalized = crate::normalize_path(&name_string);
                    (
                        normalized,
                        VfsFile::from_archive(&name_string, Arc::clone(stored_archive)),
                    )
                })
            })
            .collect()
    }
}
