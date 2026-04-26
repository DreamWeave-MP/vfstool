// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
#[cfg(feature = "serialize")]
use crate::SerializeType;
use crate::{DirectoryNode, DisplayTree, VfsFile};
#[cfg(feature = "serialize")]
use std::io::{Error, ErrorKind, Result};
use std::{
    collections::BTreeMap,
    fmt::Write,
    path::{Path, PathBuf},
};

impl VFS {
    /// Returns a sorted version of the VFS contents as a binary tree.
    #[must_use]
    pub fn tree(&self, relative: bool) -> DisplayTree {
        self.build_tree(relative, None::<&fn(&Path, &VfsFile) -> bool>)
    }

    /// Returns a sorted tree containing only files accepted by `file_filter`.
    ///
    /// Unlike the old two-pass implementation (build full tree then prune),
    /// this filters during construction: directory nodes are only created for
    /// paths that contain at least one accepted file, so no separate prune pass
    /// is required.
    ///
    /// The predicate receives the normalized relative VFS key (`&Path`) and the
    /// `&VfsFile`. Having the key available allows O(1) cross-VFS lookups inside
    /// the predicate without needing to re-derive the relative path from the
    /// absolute physical path.
    pub fn tree_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&Path, &VfsFile) -> bool,
    ) -> DisplayTree {
        self.build_tree(relative, Some(&file_filter))
    }

    fn build_tree<F: Fn(&Path, &VfsFile) -> bool>(
        &self,
        relative: bool,
        file_filter: Option<&F>,
    ) -> DisplayTree {
        let mut tree: DisplayTree = BTreeMap::new();
        let root_path: PathBuf = if relative { "Data Files" } else { "/" }.into();

        tree.insert(root_path.clone(), DirectoryNode::new());

        for (key, entry) in &self.file_map {
            let path = if relative {
                entry.parent_archive_name()
            } else {
                entry.parent_archive_path()
            }
            .map_or_else(
                || {
                    if relative {
                        key.into()
                    } else {
                        entry.path().to_path_buf()
                    }
                },
                |parent| PathBuf::from(parent).join(key),
            );

            if file_filter.as_ref().is_some_and(|f| !f(key, entry)) {
                continue;
            }

            let new_file = entry.clone();

            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(root_path.as_path());

            let mut current_path = PathBuf::new();
            let mut current_node = tree
                .get_mut(&root_path)
                .expect("Root path should be guaranteed to always exist!");

            for component in parent.components() {
                current_path.push(component);

                if current_path == root_path {
                    continue;
                }

                let component_name = PathBuf::from(component.as_os_str());
                current_node = current_node.subdirs.entry(component_name).or_default();
            }

            current_node.files.push(new_file);
        }

        tree.get_mut(&root_path)
            .expect("Root path should be guaranteed to always exist!")
            .sort();

        tree
    }

    /// String formatter for the file tree.
    fn file_str<S: AsRef<str> + std::fmt::Display>(file: S) -> String {
        format!("{}{}\n", Self::FILE_PREFIX, file)
    }

    /// String formatter for the file tree.
    fn dir_str<S: AsRef<str> + std::fmt::Display>(dir: S) -> String {
        format!("{}{}/\n", Self::DIR_PREFIX, dir)
    }

    /// Returns the formatted file tree for a filtered subset.
    ///
    /// # Panics
    ///
    /// Panics only if formatting a `String` fails, which should not occur.
    pub fn display_filtered(
        &self,
        relative: bool,
        file_filter: impl Fn(&Path, &VfsFile) -> bool,
    ) -> String {
        let tree = self.tree_filtered(relative, file_filter);
        let mut output = String::new();
        let _ = write_tree(&tree, &mut output);
        output
    }

    /// Serializes the result of `tree` or `display_filtered` functions to JSON, YAML, or TOML.
    #[cfg(feature = "serialize")]
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn serialize_from_tree(tree: &DisplayTree, write_type: SerializeType) -> Result<String> {
        fn to_io_error<E: std::fmt::Display>(err: E) -> Error {
            Error::new(ErrorKind::InvalidData, err.to_string())
        }

        let serialized_content = match write_type {
            SerializeType::Json => serde_json::to_string(&tree).map_err(to_io_error)?,
            SerializeType::Yaml => serde_yaml::to_string(&tree).map_err(to_io_error)?,
            SerializeType::Toml => toml::to_string_pretty(&tree).map_err(to_io_error)?,
        };

        Ok(serialized_content)
    }
}

fn write_node<W: Write>(w: &mut W, node: &DirectoryNode, dir: &Path) -> std::fmt::Result {
    if !node.files.is_empty() {
        write!(w, "{}", VFS::dir_str(dir.to_string_lossy()))?;
        for file in &node.files {
            write!(
                w,
                "{}",
                VFS::file_str(file.path().file_name().unwrap().to_string_lossy())
            )?;
        }
    }
    for (subdir_name, subdir_node) in &node.subdirs {
        write_node(w, subdir_node, subdir_name)?;
    }
    Ok(())
}

fn write_tree<W: Write>(tree: &DisplayTree, w: &mut W) -> std::fmt::Result {
    for (root_subdir, root_node) in tree {
        write_node(w, root_node, root_subdir)?;
    }
    Ok(())
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_tree(&self.tree(true), f)
    }
}
