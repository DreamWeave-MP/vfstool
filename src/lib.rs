pub mod directory_node;
pub mod vfs;
pub mod vfs_file;

pub use directory_node::DirectoryNode;
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
