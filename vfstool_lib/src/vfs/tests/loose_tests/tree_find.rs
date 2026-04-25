use super::*;

// ---- tree structure ----

#[test]
fn tree_relative_root_key_is_data_files() {
    let dir = TempDir::new("vfsloose_tree_relroot");
    dir.write("foo.txt", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree(true);
    assert!(tree.contains_key(&PathBuf::from("Data Files")));
}

#[test]
fn tree_absolute_root_key_is_slash() {
    let dir = TempDir::new("vfsloose_tree_absroot");
    dir.write("foo.txt", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree(false);
    assert!(tree.contains_key(&PathBuf::from("/")));
}

#[test]
fn tree_root_level_file_appears_in_root_node() {
    let dir = TempDir::new("vfsloose_tree_rootfile");
    dir.write("morrowind.esm", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree(true);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();
    assert_eq!(root.files.len(), 1);
    assert_eq!(root.files[0].file_name().unwrap(), "morrowind.esm");
}

#[test]
fn tree_nested_file_reachable_somewhere_in_tree() {
    let dir = TempDir::new("vfsloose_tree_nested");
    dir.write("textures/landscape/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree(true);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();
    let all = collect_all_filenames(root);
    assert!(all.contains(&"foo.dds".to_string()));
}

#[test]
fn tree_files_sorted_within_node() {
    let dir = TempDir::new("vfsloose_tree_sorted");
    dir.write("zoo.txt", b"");
    dir.write("alpha.txt", b"");
    dir.write("middle.txt", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree(true);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();
    let names: Vec<_> = root
        .files
        .iter()
        .filter_map(|f| f.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        names, sorted,
        "files within a DirectoryNode should be alphabetically sorted"
    );
}

#[test]
fn tree_subdir_keys_are_component_names_not_full_paths() {
    let dir = TempDir::new("vfsloose_tree_keys");
    dir.write("textures/landscape/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree(true);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();

    assert!(
        root.subdirs.contains_key(&PathBuf::from("textures")),
        "top-level subdir should have key 'textures'"
    );
    let textures = root.subdirs.get(&PathBuf::from("textures")).unwrap();
    assert!(
        textures.subdirs.contains_key(&PathBuf::from("landscape")),
        "subdir key should be 'landscape', not 'textures/landscape' — see IMPROVEMENTS.md #14"
    );
}

// ---- tree_filtered ----

#[test]
fn tree_filtered_keeps_only_matching_files() {
    let dir = TempDir::new("vfsloose_filtered_keep");
    dir.write("textures/foo.dds", b"");
    dir.write("meshes/bar.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let tree = vfs.tree_filtered(true, |_key, file| {
        file.path().extension().is_some_and(|e| e == "dds")
    });
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();
    let all = collect_all_filenames(root);

    assert!(
        all.iter().all(|f| {
            Path::new(f)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dds"))
        }),
        "only .dds files should survive the filter"
    );
    assert!(!all.is_empty());
}

#[test]
fn tree_filtered_prunes_empty_subdirs() {
    let dir = TempDir::new("vfsloose_filtered_prune");
    dir.write("textures/foo.dds", b"");
    dir.write("meshes/bar.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    // Keep only .dds — the entire meshes/ subtree should disappear
    let tree = vfs.tree_filtered(true, |_key, file| {
        file.path().extension().is_some_and(|e| e == "dds")
    });
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();

    assert!(
        !contains_nif(root),
        "empty subdirs should be pruned after filtering"
    );
}

#[test]
fn tree_filtered_all_excluded_yields_empty_root() {
    let dir = TempDir::new("vfsloose_filtered_all_gone");
    dir.write("foo.txt", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let tree = vfs.tree_filtered(true, |_, _| false);
    let root = tree.get(&PathBuf::from("Data Files")).unwrap();
    assert!(root.files.is_empty());
    assert!(root.subdirs.is_empty());
}

#[test]
fn tree_filtered_all_included_matches_full_tree() {
    let dir = TempDir::new("vfsloose_filtered_all_in");
    dir.write("a/foo.txt", b"");
    dir.write("b/bar.txt", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let full = vfs.tree(true);
    let filtered = vfs.tree_filtered(true, |_, _| true);

    let full_root = full.get(&PathBuf::from("Data Files")).unwrap();
    let filt_root = filtered.get(&PathBuf::from("Data Files")).unwrap();

    assert_eq!(
        collect_all_filenames(full_root),
        collect_all_filenames(filt_root),
    );
}

// ---- find_by_regex ----
#[test]
fn find_by_regex_matching_files_returned() {
    let dir = TempDir::new("vfs_newmethods_regex_match");
    dir.write("foo.txt", b"a");
    dir.write("bar.txt", b"b");
    dir.write("baz.nif", b"c");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let tree = vfs.find_by_regex(r"\.txt$", true).unwrap();
    let count = count_files_in_tree(&tree);
    assert_eq!(count, 2, "only .txt files should match");
}

#[test]
fn find_by_regex_preserves_regex_escapes() {
    let dir = TempDir::new("vfs_newmethods_regex_escapes");
    dir.write("textures/foo.dds", b"a");
    dir.write("textures/foo_dds", b"b");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let tree = vfs.find_by_regex(r"textures/.*\.dds$", true).unwrap();
    assert_eq!(count_files_in_tree(&tree), 1);
}

#[test]
fn find_by_regex_non_matching_excluded() {
    let dir = TempDir::new("vfs_newmethods_regex_excl");
    dir.write("alpha.dds", b"a");
    dir.write("beta.nif", b"b");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let tree = vfs.find_by_regex("[.]txt$", true).unwrap();
    let count = count_files_in_tree(&tree);
    assert_eq!(count, 0, "no .txt files should match");
}

#[test]
fn find_by_regex_does_not_match_source_directory() {
    let dir = TempDir::new("vfs_newmethods_regex_source_dir_match");
    dir.write("asset.nif", b"a");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let tree = vfs
        .find_by_regex("vfs_newmethods_regex_source_dir_match", true)
        .unwrap();
    assert_eq!(count_files_in_tree(&tree), 0);
}

#[test]
fn find_by_regex_invalid_returns_err() {
    let dir = TempDir::new("vfs_newmethods_regex_err");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.find_by_regex(r"[invalid", true).is_err());
}

#[test]
fn find_by_regex_case_insensitive() {
    let dir = TempDir::new("vfs_newmethods_regex_case");
    dir.write("foo.txt", b"a");
    dir.write("other.nif", b"b");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    // Pattern in uppercase should still match the lowercase filename
    let tree = vfs.find_by_regex("FOO", true).unwrap();
    let count = count_files_in_tree(&tree);
    assert_eq!(
        count, 1,
        "FOO pattern should match foo.txt case-insensitively"
    );
}
