use super::*;
use crate::{DirectoryNode, DisplayTree};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// RAII temp directory scoped to a named subdirectory of cwd.
/// Cleaned up on drop so panics in tests don't leave debris.
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

    /// Write `data` to `rel` (relative to this dir), creating intermediate dirs.
    fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let target = self.0.join(rel);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, data).unwrap();
        target
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Recursively collect all file names from a `DirectoryNode` tree.
fn collect_all_filenames(node: &DirectoryNode) -> Vec<String> {
    let mut names: Vec<String> = node
        .files
        .iter()
        .filter_map(|f| f.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    for sub in node.subdirs.values() {
        names.extend(collect_all_filenames(sub));
    }
    names
}

fn count_files_in_tree(tree: &DisplayTree) -> usize {
    fn count(node: &DirectoryNode) -> usize {
        node.files.len() + node.subdirs.values().map(count).sum::<usize>()
    }
    tree.values().map(count).sum()
}

fn contains_nif(node: &DirectoryNode) -> bool {
    node.files
        .iter()
        .any(|f| f.path().extension().is_some_and(|e| e == "nif"))
        || node.subdirs.values().any(contains_nif)
}

#[path = "loose_tests/construction_mutation.rs"]
mod construction_mutation;
#[path = "loose_tests/diff_directory.rs"]
mod diff_directory;
#[path = "loose_tests/extract_remaining.rs"]
mod extract_remaining;
#[path = "loose_tests/lookup_priority_query.rs"]
mod lookup_priority_query;
#[path = "loose_tests/tree_find.rs"]
mod tree_find;
