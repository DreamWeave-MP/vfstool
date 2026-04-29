use super::*;

// ---- get_file ----

#[test]
fn get_file_exact_lowercase_key() {
    let dir = TempDir::new("vfsloose_get_exact");
    dir.write("meshes/foo.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.get_file("meshes/foo.nif").is_some());
}

#[test]
fn get_file_case_insensitive() {
    let dir = TempDir::new("vfsloose_get_case");
    dir.write("meshes/foo.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.get_file("Meshes/Foo.NIF").is_some());
    assert!(vfs.get_file("MESHES/FOO.NIF").is_some());
    assert!(vfs.get_file("mEsHeS/fOo.nIf").is_some());
}

#[test]
fn get_file_backslash_lookup() {
    let dir = TempDir::new("vfsloose_get_backslash");
    dir.write("meshes/foo.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.get_file("meshes\\foo.nif").is_some());
    assert!(vfs.get_file("Meshes\\Foo.NIF").is_some());
}

#[test]
fn get_file_nonexistent_returns_none() {
    let dir = TempDir::new("vfsloose_get_none");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.get_file("does_not_exist.txt").is_none());
}

#[test]
fn get_file_path_confirmed_correct() {
    // get_file must return the actual on-disk path, not the normalized key
    let dir = TempDir::new("vfsloose_get_path");
    let written = dir.write("Meshes/XBase_Anim.NIF", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    let file = vfs.get_file("meshes/xbase_anim.nif").unwrap();
    assert_eq!(file.path(), written);
}

// ---- priority / collision ----

/// Core invariant: later directory in the list overrides earlier one.
/// This mirrors `OpenMW`'s data= ordering semantics.
#[test]
fn later_dir_wins_over_earlier_for_same_file() {
    let dir1 = TempDir::new("vfsprio_later_wins_dir1");
    let dir2 = TempDir::new("vfsprio_later_wins_dir2");
    let path1 = dir1.write("shared.txt", b"from_dir1");
    let path2 = dir2.write("shared.txt", b"from_dir2");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

    assert_eq!(
        vfs.iter().count(),
        1,
        "collision should collapse to one entry"
    );
    let winner = vfs.get_file("shared.txt").unwrap();
    assert_eq!(
        winner.path(),
        path2,
        "dir2 (later) should override dir1 (earlier)"
    );
    assert_ne!(winner.path(), path1);
}

#[test]
fn earlier_dir_does_not_win_over_later_dir() {
    let dir1 = TempDir::new("vfsprio_earlier_loses_dir1");
    let dir2 = TempDir::new("vfsprio_earlier_loses_dir2");
    dir1.write("shared.txt", b"loser");
    let path2 = dir2.write("shared.txt", b"winner");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);
    assert_eq!(vfs.get_file("shared.txt").unwrap().path(), path2);
}

#[test]
fn three_dirs_last_one_wins() {
    let dir1 = TempDir::new("vfsprio_three_dir1");
    let dir2 = TempDir::new("vfsprio_three_dir2");
    let dir3 = TempDir::new("vfsprio_three_dir3");
    dir1.write("shared.txt", b"1");
    dir2.write("shared.txt", b"2");
    let path3 = dir3.write("shared.txt", b"3");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path(), dir3.path()], None);
    assert_eq!(vfs.iter().count(), 1);
    assert_eq!(vfs.get_file("shared.txt").unwrap().path(), path3);
}

#[test]
fn partial_overlap_unique_files_present_and_shared_resolves_to_later() {
    let dir1 = TempDir::new("vfsprio_partial_dir1");
    let dir2 = TempDir::new("vfsprio_partial_dir2");
    let only1 = dir1.write("only_in_1.txt", b"1");
    dir1.write("shared.txt", b"from_dir1");
    let only2 = dir2.write("only_in_2.txt", b"2");
    let shared2 = dir2.write("shared.txt", b"from_dir2");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

    assert_eq!(vfs.iter().count(), 3, "3 unique VFS paths expected");
    assert_eq!(vfs.get_file("only_in_1.txt").unwrap().path(), only1);
    assert_eq!(vfs.get_file("only_in_2.txt").unwrap().path(), only2);
    assert_eq!(vfs.get_file("shared.txt").unwrap().path(), shared2);
}

/// Case-folding means paths differing only in case collide on the same VFS
/// key — the usual priority rules still apply.
#[test]
fn collision_across_dirs_via_case_normalization() {
    let dir1 = TempDir::new("vfsprio_case_dir1");
    let dir2 = TempDir::new("vfsprio_case_dir2");
    dir1.write("Textures/Foo.DDS", b"dir1");
    let path2 = dir2.write("textures/foo.dds", b"dir2");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

    assert_eq!(
        vfs.iter().count(),
        1,
        "case variants are the same VFS entry"
    );
    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), path2);
}

#[test]
fn same_source_duplicate_providers_report_distinct_original_paths() {
    let dir = TempDir::new("vfsprio_same_source_duplicate_providers");
    let upper = dir.write("upper/Foo.DDS", b"upper");
    let lower = dir.write("lower/foo.dds", b"lower");

    let source = SourceMeta {
        path: dir.path().to_path_buf(),
        kind: crate::SourceKind::LooseDir,
    };
    let key = NormalizedPath::new(b"textures/foo.dds");
    let mut vfs = VFS::new();
    assert_eq!(
        vfs.push_provider_batch(
            &source,
            [
                (key.clone(), VfsFile::from(&upper)),
                (key, VfsFile::from(&lower))
            ],
        ),
        2
    );

    assert_eq!(vfs.iter().count(), 1);
    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), lower);

    let providers = vfs.provider_records_for("textures/foo.dds");
    assert_eq!(providers.len(), 2);
    let original_paths = providers
        .iter()
        .map(|provider| provider.original_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(original_paths.contains(Path::new("upper/Foo.DDS")));
    assert!(original_paths.contains(Path::new("lower/foo.dds")));
}

/// Override must be per-key: files unique to an earlier dir must survive
/// even when later dirs override other keys.
#[test]
fn override_is_per_key_not_per_directory() {
    let dir1 = TempDir::new("vfsprio_perkey_dir1");
    let dir2 = TempDir::new("vfsprio_perkey_dir2");
    let keep = dir1.write("unique_to_dir1.txt", b"keep");
    dir1.write("shared.txt", b"dir1");
    dir2.write("shared.txt", b"dir2");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

    assert_eq!(vfs.iter().count(), 2);
    assert_eq!(vfs.get_file("unique_to_dir1.txt").unwrap().path(), keep);
}

/// Lookup with backslash separator must resolve to the same entry as
/// forward-slash, and priority still applies.
#[test]
fn backslash_lookup_finds_overriding_file() {
    let dir1 = TempDir::new("vfsprio_bslash_dir1");
    let dir2 = TempDir::new("vfsprio_bslash_dir2");
    dir1.write("meshes/xbase.nif", b"dir1");
    let path2 = dir2.write("meshes/xbase.nif", b"dir2");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

    assert_eq!(vfs.iter().count(), 1);
    assert_eq!(vfs.get_file("meshes\\xbase.nif").unwrap().path(), path2);
}

// ---- paths_matching ----

#[test]
fn paths_matching_finds_by_substring() {
    let dir = TempDir::new("vfsloose_matching");
    dir.write("textures/landscape/foo.dds", b"");
    dir.write("textures/sky/bar.dds", b"");
    dir.write("meshes/actors/baz.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert_eq!(vfs.paths_matching("textures").count(), 2);
    assert_eq!(vfs.paths_matching("meshes").count(), 1);
}

#[test]
fn paths_matching_normalizes_query() {
    let dir = TempDir::new("vfsloose_matching_case");
    dir.write("textures/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    // Uppercase query normalized before matching
    assert_eq!(vfs.paths_matching("TEXTURES").count(), 1);
    assert_eq!(vfs.paths_matching("Textures").count(), 1);
}

#[test]
fn paths_matching_no_match_returns_empty() {
    let dir = TempDir::new("vfsloose_matching_empty");
    dir.write("meshes/foo.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert_eq!(vfs.paths_matching("textures").count(), 0);
}

// ---- paths_with ----

#[test]
fn paths_with_finds_all_under_prefix() {
    let dir = TempDir::new("vfsloose_with");
    dir.write("textures/landscape/a.dds", b"");
    dir.write("textures/landscape/b.dds", b"");
    dir.write("textures/sky/c.dds", b"");
    dir.write("meshes/foo.nif", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    assert_eq!(vfs.paths_with("textures").count(), 3);
    assert_eq!(vfs.paths_with("textures/landscape").count(), 2);
    assert_eq!(vfs.paths_with("meshes").count(), 1);
}

#[test]
fn paths_with_uses_path_component_boundaries() {
    let dir = TempDir::new("vfsloose_with_boundaries");
    dir.write("textures/foo.dds", b"");
    dir.write("textures2/bar.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);

    let paths = vfs
        .paths_with("textures")
        .map(|(key, _)| crate::paths::key_to_string_lossy(key))
        .collect::<Vec<_>>();

    assert_eq!(paths, vec!["textures/foo.dds"]);
}

#[test]
fn paths_with_returns_empty_for_nonexistent_prefix() {
    let dir = TempDir::new("vfsloose_with_none");
    dir.write("textures/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert_eq!(vfs.paths_with("sounds").count(), 0);
}

// ---- contains ----

#[test]
fn contains_true_for_present_relative_key() {
    let dir = TempDir::new("vfsloose_contains_true");
    dir.write("textures/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.contains(Path::new("textures/foo.dds")));
}

#[test]
fn contains_normalizes_before_lookup() {
    let dir = TempDir::new("vfsloose_contains_norm");
    dir.write("textures/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.contains(Path::new("Textures\\FOO.DDS")));
}

#[test]
fn contains_false_for_absent_key() {
    let dir = TempDir::new("vfsloose_contains_false");
    dir.write("textures/foo.dds", b"");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(!vfs.contains(Path::new("textures/bar.dds")));
}
