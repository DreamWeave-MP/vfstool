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
    use super::{VFS, VfsFile};
    use bsatoollib::BSAFile;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    pub type ArchiveList = Vec<Arc<BSAFile>>;

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
            .iter()
            .filter_map(|archive| file_map.get(&PathBuf::from(archive.to_ascii_lowercase())))
            .filter_map(|valid_archive| BSAFile::from(valid_archive.path().to_string_lossy()).ok())
            .map(Arc::new)
            .collect()
    }

    pub fn file_map(archives: &ArchiveList) -> HashMap<PathBuf, VfsFile> {
        archives
            .iter()
            .flat_map(|archive| {
                archive.get_list().into_iter().map(|file_struct| {
                    (
                        PathBuf::from(&file_struct.name),
                        VfsFile::from_archive(&file_struct.name, Arc::clone(archive)),
                    )
                })
            })
            .collect()
    }
}
