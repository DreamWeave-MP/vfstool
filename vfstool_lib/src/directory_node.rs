use crate::{DisplayTree, VfsFile};
use std::collections::BTreeMap;

#[cfg(feature = "serialize")]
use serde::{Serialize, Serializer, ser::SerializeMap};

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
/// use vfstool_lib::{directory_node::DirectoryNode, VfsFile};
///
/// let mut node = DirectoryNode::new();
///
/// let file = VfsFile::from("test.txt");
/// node.files.push(file);
///
/// let mut subdir = DirectoryNode::new();
/// subdir.files.push(VfsFile::from("nested.txt"));
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
    /// use vfstool_lib::{directory_node::DirectoryNode, VfsFile};
    ///
    /// let mut node = DirectoryNode::new();
    ///
    /// node.files.push(VfsFile::from("keep.txt"));
    /// node.files.push(VfsFile::from("remove.txt"));
    ///
    /// node.filter(&|file: &VfsFile| file.file_name().is_some_and(|n| n == "keep.txt"));
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

#[cfg(feature = "serialize")]
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
                    .map(|file| file.to_string_lossy())
                    .collect::<Vec<std::borrow::Cow<'_, str>>>(),
            )?;
        }

        for (dir_name, subdir) in &self.subdirs {
            let dir_key = dir_name.file_name().unwrap_or_default().to_string_lossy();

            map.serialize_entry(&dir_key, subdir)?;
        }

        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    use serde_yaml;
    use std::path::PathBuf;
    use toml;

    fn sample_directory_node() -> DirectoryNode {
        let mut root = DirectoryNode::new();

        for i in 1..=3 {
            let mut subdir = DirectoryNode::new();

            // Add three files to the subdir
            for j in 1..=3 {
                subdir
                    .files
                    .push(VfsFile::from(format!("file{}_{}.txt", i, j)));
            }

            // Create a child subdirectory inside this subdir
            let mut child_subdir = DirectoryNode::new();
            for k in 1..=3 {
                child_subdir
                    .files
                    .push(VfsFile::from(format!("nested_file{}_{}.txt", i, k)));
            }

            subdir
                .subdirs
                .insert(format!("child_subdir{}", i).into(), child_subdir);

            root.subdirs.insert(format!("subdir{}", i).into(), subdir);
        }

        root
    }

    #[test]
    fn serialize_to_json() {
        let node = sample_directory_node();
        let json_output = serde_json::to_string_pretty(&node).expect("JSON serialization failed");

        println!("{}", &json_output);

        let expected = r#"{
  "subdir1": {
    ".": [
      "file1_1.txt",
      "file1_2.txt",
      "file1_3.txt"
    ],
    "child_subdir1": {
      ".": [
        "nested_file1_1.txt",
        "nested_file1_2.txt",
        "nested_file1_3.txt"
      ]
    }
  },
  "subdir2": {
    ".": [
      "file2_1.txt",
      "file2_2.txt",
      "file2_3.txt"
    ],
    "child_subdir2": {
      ".": [
        "nested_file2_1.txt",
        "nested_file2_2.txt",
        "nested_file2_3.txt"
      ]
    }
  },
  "subdir3": {
    ".": [
      "file3_1.txt",
      "file3_2.txt",
      "file3_3.txt"
    ],
    "child_subdir3": {
      ".": [
        "nested_file3_1.txt",
        "nested_file3_2.txt",
        "nested_file3_3.txt"
      ]
    }
  }
}"#;

        assert_eq!(json_output, expected);
    }

    #[test]
    fn serialize_to_toml() {
        let node = sample_directory_node();
        let toml_output = toml::to_string_pretty(&node).expect("TOML serialization failed");

        println!("{}", &toml_output);
        let expected = r#"[subdir1]
"." = [
    "file1_1.txt",
    "file1_2.txt",
    "file1_3.txt",
]

[subdir1.child_subdir1]
"." = [
    "nested_file1_1.txt",
    "nested_file1_2.txt",
    "nested_file1_3.txt",
]

[subdir2]
"." = [
    "file2_1.txt",
    "file2_2.txt",
    "file2_3.txt",
]

[subdir2.child_subdir2]
"." = [
    "nested_file2_1.txt",
    "nested_file2_2.txt",
    "nested_file2_3.txt",
]

[subdir3]
"." = [
    "file3_1.txt",
    "file3_2.txt",
    "file3_3.txt",
]

[subdir3.child_subdir3]
"." = [
    "nested_file3_1.txt",
    "nested_file3_2.txt",
    "nested_file3_3.txt",
]
"#;

        assert_eq!(toml_output, expected);
    }

    #[test]
    fn serialize_to_yaml() {
        let node = sample_directory_node();
        let yaml_output =
            serde_yaml::to_string(&node).expect("YAML serialization failed");

        println!("{}", &yaml_output);

        let expected = "subdir1:\n  .:\n  - file1_1.txt\n  - file1_2.txt\n  - file1_3.txt\n  child_subdir1:\n    .:\n    - nested_file1_1.txt\n    - nested_file1_2.txt\n    - nested_file1_3.txt\nsubdir2:\n  .:\n  - file2_1.txt\n  - file2_2.txt\n  - file2_3.txt\n  child_subdir2:\n    .:\n    - nested_file2_1.txt\n    - nested_file2_2.txt\n    - nested_file2_3.txt\nsubdir3:\n  .:\n  - file3_1.txt\n  - file3_2.txt\n  - file3_3.txt\n  child_subdir3:\n    .:\n    - nested_file3_1.txt\n    - nested_file3_2.txt\n    - nested_file3_3.txt\n";

        assert_eq!(yaml_output, expected);
    }

    #[test]
    fn test_directory_node_filter() {
        let mut root = sample_directory_node();

        root.filter(&|file| file.file_name().map_or(false, |name| name.to_string_lossy().contains('2')));

        assert_eq!(
            root.subdirs.len(),
            3,
            "Each subdirectory should have at least one file with the number 2 in its root"
        );

        let subdirs = ["subdir1", "subdir2", "subdir3"];
        for &subdir in &subdirs {
            assert!(
                root.subdirs.contains_key(&PathBuf::from(&subdir)),
                "{subdir} should still be present"
            );
        }

        // Validate subdir1
        let subdir1 = root
            .subdirs
            .get(&PathBuf::from("subdir1"))
            .expect("subdir1 should exist");
        assert_eq!(
            subdir1.files.len(),
            1,
            "subdir1 should have exactly one file."
        );

        let child_subdir1 = subdir1
            .subdirs
            .get(&PathBuf::from("child_subdir1"))
            .expect("child_subdir1 should still exist");
        assert_eq!(
            child_subdir1.files.len(),
            1,
            "child_subdir1 should have exactly one file."
        );

        // Validate subdir2
        let subdir2 = root
            .subdirs
            .get(&PathBuf::from("subdir2"))
            .expect("subdir2 should exist");
        assert_eq!(
            subdir2.files.len(),
            3,
            "subdir2 should have exactly three files with '2' in their names."
        );

        let child_subdir2 = subdir2
            .subdirs
            .get(&PathBuf::from("child_subdir2"))
            .expect("child_subdir2 should still exist");
        assert_eq!(
            child_subdir2.files.len(),
            3,
            "child_subdir2 should have exactly three files with '2' in their names."
        );

        // Validate subdir3
        let subdir3 = root
            .subdirs
            .get(&PathBuf::from("subdir3"))
            .expect("subdir3 should exist");
        assert_eq!(
            subdir3.files.len(),
            1,
            "subdir3 should have exactly one file."
        );

        let child_subdir3 = subdir3
            .subdirs
            .get(&PathBuf::from("child_subdir3"))
            .expect("child_subdir3 should still exist");
        assert_eq!(
            child_subdir3.files.len(),
            1,
            "child_subdir3 should have exactly one file."
        );
    }

    // ---- DirectoryNode::new / Default ----

    #[test]
    fn new_produces_empty_node() {
        let node = DirectoryNode::new();
        assert!(node.files.is_empty());
        assert!(node.subdirs.is_empty());
    }

    // ---- DirectoryNode::sort ----

    #[test]
    fn sort_alphabetizes_files_in_node() {
        let mut node = DirectoryNode::new();
        for name in ["zoo.txt", "alpha.txt", "middle.txt"] {
            node.files.push(VfsFile::from(name));
        }
        node.sort();
        let names: Vec<_> = node
            .files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alpha.txt", "middle.txt", "zoo.txt"]);
    }

    #[test]
    fn sort_is_recursive_into_subdirs() {
        let mut root = DirectoryNode::new();
        let mut sub = DirectoryNode::new();
        for name in ["z.txt", "a.txt", "m.txt"] {
            sub.files.push(VfsFile::from(name));
        }
        root.subdirs.insert("sub".into(), sub);
        root.sort();

        let sub = root.subdirs.get(&PathBuf::from("sub")).unwrap();
        let names: Vec<_> = sub
            .files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.txt", "m.txt", "z.txt"]);
    }

    #[test]
    fn sort_empty_node_does_not_panic() {
        let mut node = DirectoryNode::new();
        node.sort(); // should not panic
    }

    // ---- DirectoryNode::filter ----

    #[test]
    fn filter_keeps_files_matching_predicate() {
        let mut node = DirectoryNode::new();
        node.files.push(VfsFile::from("keep.txt"));
        node.files.push(VfsFile::from("drop.nif"));
        node.filter(&|f| f.path().extension().is_some_and(|e| e == "txt"));
        assert_eq!(node.files.len(), 1);
        assert_eq!(node.files[0].file_name().unwrap(), "keep.txt");
    }

    #[test]
    fn filter_removes_all_when_predicate_always_false() {
        let mut node = DirectoryNode::new();
        node.files.push(VfsFile::from("a.txt"));
        node.files.push(VfsFile::from("b.txt"));
        node.filter(&|_| false);
        assert!(node.files.is_empty());
    }

    #[test]
    fn filter_keeps_all_when_predicate_always_true() {
        let mut node = DirectoryNode::new();
        node.files.push(VfsFile::from("a.txt"));
        node.files.push(VfsFile::from("b.txt"));
        node.filter(&|_| true);
        assert_eq!(node.files.len(), 2);
    }

    #[test]
    fn filter_prunes_subdir_that_becomes_empty() {
        let mut root = DirectoryNode::new();
        let mut sub = DirectoryNode::new();
        sub.files.push(VfsFile::from("dropped.nif"));
        root.subdirs.insert("meshes".into(), sub);

        root.filter(&|f| f.path().extension().is_some_and(|e| e == "dds"));

        assert!(
            root.subdirs.is_empty(),
            "a subdir with no surviving files should be pruned"
        );
    }

    #[test]
    fn filter_keeps_subdir_that_still_has_files() {
        let mut root = DirectoryNode::new();
        let mut sub = DirectoryNode::new();
        sub.files.push(VfsFile::from("kept.dds"));
        sub.files.push(VfsFile::from("dropped.nif"));
        root.subdirs.insert("textures".into(), sub);

        root.filter(&|f| f.path().extension().is_some_and(|e| e == "dds"));

        assert_eq!(root.subdirs.len(), 1, "non-empty subdir should survive");
        let textures = root.subdirs.get(&PathBuf::from("textures")).unwrap();
        assert_eq!(textures.files.len(), 1);
    }

    #[test]
    fn filter_is_recursive_through_nested_subdirs() {
        let mut root = DirectoryNode::new();
        let mut outer = DirectoryNode::new();
        let mut inner = DirectoryNode::new();
        inner.files.push(VfsFile::from("deep.nif"));
        outer.subdirs.insert("inner".into(), inner);
        root.subdirs.insert("outer".into(), outer);

        root.filter(&|f| f.path().extension().is_some_and(|e| e == "dds"));

        // outer had only inner, inner had only a .nif — both should be pruned
        assert!(root.subdirs.is_empty(), "deeply nested empty subtrees should be pruned");
    }
}
