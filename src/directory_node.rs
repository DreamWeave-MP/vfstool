use crate::{DisplayTree, VfsFile};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::collections::BTreeMap;

/// Represents a directory node in the Virtual File System (VFS).
///
/// A `DirectoryNode` contains:
/// - A list of files (`files`).
/// - A map of subdirectories (`subdirs`), where each key is a directory name.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeMap;
/// use vfstool::{directory_node::DirectoryNode, VfsFile};
///
/// let mut node = DirectoryNode::new();
///
/// let file = VfsFile::new("test.txt".into());
/// node.files.push(file);
///
/// let mut subdir = DirectoryNode::new();
/// subdir.files.push(VfsFile::new("nested.txt".into()));
///
/// node.subdirs.insert("sub".into(), subdir);
///
/// assert_eq!(node.subdirs.len(), 1);
/// assert_eq!(node.files.len(), 1);
/// ```
///
/// The `sort` and `filter` methods allow organizing and modifying the directory contents.
#[derive(Debug)]
pub struct DirectoryNode {
    pub files: Vec<VfsFile>,
    pub subdirs: DisplayTree,
}

impl DirectoryNode {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            subdirs: BTreeMap::new(),
        }
    }

    /// Sorts the files in the directory by name and recursively sorts subdirectories.
    ///
    /// This ensures files appear in a consistent order.
    /// Useful when serializing or displaying directory contents.
    pub fn sort(&mut self) {
        self.files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        self.subdirs.values_mut().for_each(|dir| dir.sort());
    }

    /// Filters the directory's files based on a predicate and removes empty subdirectories.
    ///
    /// # Arguments
    ///
    /// * `file_filter` - A function that takes a reference to `Arc<VfsFile>`
    ///   and returns `true` if the file should be kept, or `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::ffi::OsStr;
    /// use vfstool::{DirectoryNode, VfsFile};
    ///
    /// let mut node = DirectoryNode::new();
    ///
    /// node.files.push(VfsFile::new("keep.txt".into()));
    /// node.files.push(VfsFile::new("remove.txt".into()));
    ///
    /// node.filter(&|file| file.file_name() == Some("keep.txt"));
    ///
    /// assert_eq!(node.files.len(), 1);
    /// ```
    pub fn filter<F>(&mut self, file_filter: &F)
    where
        F: Fn(&VfsFile) -> bool,
    {
        self.files.retain(file_filter);
        self.subdirs.retain(|_path, subdir| {
            subdir.filter(file_filter);
            !subdir.files.is_empty() || !subdir.subdirs.is_empty()
        });
    }
}

impl Serialize for DirectoryNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(
            self.subdirs.len() + if self.files.is_empty() { 0 } else { 1 },
        ))?;

        if !self.files.is_empty() {
            map.serialize_entry(
                ".",
                &self
                    .files
                    .iter()
                    .filter_map(|file| file.file_name())
                    .collect::<Vec<&str>>(),
            )?;
        }

        for (dir_name, subdir) in &self.subdirs {
            let dir_key = dir_name.file_name().unwrap_or_default().to_string_lossy();

            map.serialize_entry(&dir_key, subdir)?;
        }

        map.end()
    }
}
