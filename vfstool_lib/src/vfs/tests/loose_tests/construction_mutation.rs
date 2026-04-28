use super::*;

// ---- Construction ----

#[test]
fn from_empty_directory_yields_empty_vfs() {
    let dir = TempDir::new("vfsloose_empty");
    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert_eq!(vfs.iter().count(), 0);
}

#[test]
fn from_single_directory_all_files_present() {
    let dir = TempDir::new("vfsloose_single");
    dir.write("foo.txt", b"a");
    dir.write("bar.txt", b"b");
    dir.write("sub/baz.txt", b"c");

    let vfs = VFS::from_directories(vec![dir.path()], None);

    assert!(vfs.get_file("foo.txt").is_some());
    assert!(vfs.get_file("bar.txt").is_some());
    assert!(vfs.get_file("sub/baz.txt").is_some());
    assert_eq!(vfs.iter().count(), 3);
}

#[test]
fn try_from_directories_reports_missing_directory() {
    let dir = TempDir::new("vfsloose_missing_strict");
    let missing = dir.path().join("missing");

    let Err(err) = VFS::try_from_directories([missing.as_path()], None) else {
        panic!("strict construction should reject a missing directory");
    };

    assert!(matches!(err, VfsBuildError::Traversal { .. }));
    assert!(err.to_string().contains("failed to traverse"));
}

#[test]
#[cfg(not(any(feature = "beth-archives", feature = "zip")))]
fn try_from_directories_rejects_archives_without_archive_features() {
    let dir = TempDir::new("vfsloose_archives_without_features");

    let Err(err) = VFS::try_from_directories([dir.path()], Some(vec!["missing.zip"])) else {
        panic!("strict construction should reject requested archives when archive support is off");
    };

    assert!(matches!(err, VfsBuildError::ArchiveLoad { .. }));
    assert!(err.to_string().contains("archive support is not enabled"));
}

#[test]
#[cfg(unix)]
fn try_from_directories_reports_unreadable_traversal_entries() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new("vfsloose_unreadable_strict");
    dir.write("visible.txt", b"visible");
    let unreadable = dir.path().join("locked");
    fs::create_dir(&unreadable).unwrap();
    let original_permissions = fs::metadata(&unreadable).unwrap().permissions();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    let Err(err) = VFS::try_from_directories([dir.path()], None) else {
        panic!("strict construction should reject unreadable traversal entries");
    };

    fs::set_permissions(&unreadable, original_permissions).unwrap();
    assert!(matches!(err, VfsBuildError::Traversal { path, .. } if path == unreadable));
}

#[test]
fn set_winner_loose_file_normalizes_key_and_returns_previous_winner() {
    let dir = TempDir::new("vfsloose_insert_file");
    let first = dir.write("first.txt", b"a");
    let second = dir.write("second.txt", b"b");
    let mut vfs = VFS::new();

    assert!(
        vfs.set_winner_loose_file("Textures/Foo.dds", &first)
            .is_none()
    );
    let previous = vfs
        .set_winner_loose_file("textures/foo.dds", &second)
        .expect("second insert should return previous winner");

    assert_eq!(previous.path(), first);
    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), second);
    assert_eq!(vfs.iter().count(), 1);
}

#[test]
fn set_winner_loose_file_rejects_unsafe_keys() {
    let dir = TempDir::new("vfsloose_insert_unsafe");
    let path = dir.write("source.txt", b"a");
    let mut vfs = VFS::new();

    assert!(vfs.set_winner_loose_file("../escape.txt", &path).is_none());
    assert!(vfs.set_winner_loose_file("/absolute.txt", &path).is_none());
    assert!(
        vfs.set_winner_loose_file("C:\\absolute.txt", &path)
            .is_none()
    );
    assert_eq!(vfs.iter().count(), 0);
}

#[test]
fn push_provider_batch_groups_entries_under_one_source() {
    let dir = TempDir::new("vfsloose_batch_provider");
    let first = dir.write("first.txt", b"a");
    let second = dir.write("second.txt", b"b");
    let mut vfs = VFS::new();
    let source = SourceMeta {
        path: dir.path().to_path_buf(),
        kind: crate::SourceKind::LooseDir,
    };

    let inserted = vfs.push_provider_batch(
        &source,
        [
            (NormalizedPath::new(b"first.txt"), VfsFile::from(&first)),
            (NormalizedPath::new(b"second.txt"), VfsFile::from(&second)),
        ],
    );

    assert_eq!(inserted, 2);
    assert_eq!(vfs.get_file("first.txt").unwrap().path(), first);
    assert_eq!(vfs.get_file("second.txt").unwrap().path(), second);
    assert_eq!(vfs.layer_index().sources.len(), 1);
}

#[test]
fn push_provider_batch_rejects_unsafe_normalized_keys() {
    let dir = TempDir::new("vfsloose_batch_provider_unsafe");
    let unsafe_file = dir.write("unsafe.txt", b"unsafe");
    let safe_file = dir.write("safe.txt", b"safe");
    let mut vfs = VFS::new();
    let source = SourceMeta {
        path: dir.path().to_path_buf(),
        kind: crate::SourceKind::LooseDir,
    };

    let inserted = vfs.push_provider_batch(
        &source,
        [
            (
                NormalizedPath::new(b"../unsafe.txt"),
                VfsFile::from(&unsafe_file),
            ),
            (NormalizedPath::new(b"safe.txt"), VfsFile::from(&safe_file)),
        ],
    );

    assert_eq!(inserted, 1);
    assert!(vfs.get_file("safe.txt").is_some());
    assert!(vfs.get_file("../unsafe.txt").is_none());
}

#[test]
fn push_provider_batch_all_unsafe_entries_is_noop() {
    let dir = TempDir::new("vfsloose_batch_provider_all_unsafe");
    let unsafe_file = dir.write("unsafe.txt", b"unsafe");
    let mut vfs = VFS::new();
    let source = SourceMeta {
        path: dir.path().to_path_buf(),
        kind: crate::SourceKind::LooseDir,
    };

    let inserted = vfs.push_provider_batch(
        &source,
        [(
            NormalizedPath::new(b"../unsafe.txt"),
            VfsFile::from(&unsafe_file),
        )],
    );

    assert_eq!(inserted, 0);
    assert!(vfs.layer_index().sources.is_empty());
}

#[test]
#[cfg(unix)]
fn from_directories_skips_filenames_that_normalize_to_unsafe_keys() {
    let dir = TempDir::new("vfsloose_scan_unsafe_keys");
    dir.write("..\\outside.txt", b"escape");
    dir.write("\\absolute.txt", b"absolute");
    dir.write("safe.txt", b"safe");

    let vfs = VFS::from_directories([dir.path()], None);

    assert!(vfs.contains(Path::new("safe.txt")));
    assert!(!vfs.contains(Path::new("../outside.txt")));
    assert!(!vfs.contains(Path::new("/absolute.txt")));
    assert_eq!(vfs.iter().count(), 1);
}

#[test]
fn remove_resolved_file_normalizes_key() {
    let dir = TempDir::new("vfsloose_remove_file");
    let path = dir.write("foo.txt", b"a");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("textures/foo.dds", &path);

    let removed = vfs
        .remove_resolved_file("Textures\\Foo.dds")
        .expect("normalized key should be removed");

    assert_eq!(removed.path(), path);
    assert!(!vfs.contains(Path::new("textures/foo.dds")));
}

#[test]
fn remove_provider_prefix_removes_provider_stacks_under_prefix() {
    let dir = TempDir::new("vfsloose_remove_prefix");
    let tex = dir.write("foo.dds", b"a");
    let mesh = dir.write("foo.nif", b"b");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("textures/foo.dds", &tex);
    vfs.set_winner_loose_file("textures/nested/bar.dds", &tex);
    vfs.set_winner_loose_file("meshes/foo.nif", &mesh);

    let removed = vfs.remove_provider_prefix("Textures");

    assert_eq!(removed.len(), 2);
    assert!(!vfs.contains(Path::new("textures/foo.dds")));
    assert!(vfs.contains(Path::new("meshes/foo.nif")));
}

#[test]
fn remove_provider_prefix_uses_path_component_boundaries() {
    let dir = TempDir::new("vfsloose_remove_prefix_boundaries");
    let tex = dir.write("foo.dds", b"a");
    let tex2 = dir.write("bar.dds", b"b");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("textures/foo.dds", &tex);
    vfs.set_winner_loose_file("textures2/bar.dds", &tex2);

    let removed = vfs.remove_provider_prefix("textures");

    assert_eq!(removed.len(), 1);
    assert!(!vfs.contains(Path::new("textures/foo.dds")));
    assert!(vfs.contains(Path::new("textures2/bar.dds")));
}

#[test]
fn remove_resolved_prefix_uses_path_component_boundaries() {
    let dir = TempDir::new("vfsloose_remove_resolved_prefix_boundaries");
    let tex = dir.write("foo.dds", b"a");
    let tex2 = dir.write("bar.dds", b"b");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("textures/foo.dds", &tex);
    vfs.set_winner_loose_file("textures2/bar.dds", &tex2);

    let removed = vfs.remove_resolved_prefix("textures");

    assert_eq!(removed.len(), 1);
    assert!(!vfs.contains(Path::new("textures/foo.dds")));
    assert!(vfs.contains(Path::new("textures2/bar.dds")));
}

#[test]
fn materialization_plan_uses_path_component_conflicts() {
    let dir = TempDir::new("vfsloose_materialization_plan_boundaries");
    let foo = dir.write("foo", b"a");
    let foobar = dir.write("foobar", b"b");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("foo", &foo);
    vfs.set_winner_loose_file("foobar", &foobar);

    let out = TempDir::new("vfsloose_materialization_plan_boundaries_out");
    let plan = vfs.materialization_plan(
        out.path(),
        &crate::CollapseOptions {
            allow_copying: false,
            extract_archives: false,
            use_symlinks: false,
        },
    );

    assert!(plan.issues.is_empty());
    assert_eq!(plan.actions.len(), 2);
}

#[test]
fn materialization_plan_reports_hardlink_when_copy_is_only_fallback() {
    let dir = TempDir::new("vfsloose_materialization_plan_hardlink_fallback");
    let source = dir.write("file.txt", b"a");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("file.txt", &source);

    let out = TempDir::new("vfsloose_materialization_plan_hardlink_fallback_out");
    let plan = vfs.materialization_plan(
        out.path(),
        &crate::CollapseOptions {
            allow_copying: true,
            extract_archives: false,
            use_symlinks: false,
        },
    );

    assert!(matches!(
        plan.actions.as_slice(),
        [crate::MaterializationAction::Hardlink { .. }]
    ));
}

#[test]
fn validate_reports_case_collisions() {
    let dir = TempDir::new("vfsloose_validate_case_collision");
    dir.write("Textures/Foo.DDS", b"upper");
    dir.write("textures/foo.dds", b"lower");

    let vfs = VFS::from_directories([dir.path()], None);
    let report = vfs.validate();

    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        crate::ValidationIssue::CaseCollision { key, providers }
            if key == Path::new("textures/foo.dds") && providers.len() == 2
    )));
}

#[test]
fn remove_resolved_file_compacts_layer_sources() {
    let dir = TempDir::new("vfsloose_remove_resolved_compacts_sources");
    dir.write("only.txt", b"only");
    let mut vfs = VFS::from_directories([dir.path()], None);

    assert_eq!(vfs.layer_index().sources.len(), 1);
    assert!(vfs.remove_resolved_file("only.txt").is_some());

    assert!(vfs.layer_index().sources.is_empty());
}

#[test]
fn materialization_plan_rejects_file_directory_conflict() {
    let dir = TempDir::new("vfsloose_materialization_plan_conflict");
    let foo = dir.write("foo", b"a");
    let child = dir.write("child", b"b");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("foo", &foo);
    vfs.set_winner_loose_file("foo/bar", &child);

    let out = TempDir::new("vfsloose_materialization_plan_conflict_out");
    let plan = vfs.materialization_plan(
        out.path(),
        &crate::CollapseOptions {
            allow_copying: false,
            extract_archives: false,
            use_symlinks: false,
        },
    );

    assert_eq!(plan.issues.len(), 1);
    assert_eq!(plan.actions.len(), 1);
}

#[test]
fn materialization_plan_finds_non_adjacent_file_directory_conflict() {
    let dir = TempDir::new("vfsloose_materialization_plan_non_adjacent_conflict");
    let file = dir.write("file_source", b"a");
    let sibling = dir.write("sibling_source", b"b");
    let child = dir.write("child_source", b"c");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("a", &file);
    vfs.set_winner_loose_file("a.b", &sibling);
    vfs.set_winner_loose_file("a/c", &child);

    let out = TempDir::new("vfsloose_materialization_plan_non_adjacent_conflict_out");
    let plan = vfs.materialization_plan(
        out.path(),
        &crate::CollapseOptions {
            allow_copying: false,
            extract_archives: false,
            use_symlinks: false,
        },
    );

    assert_eq!(plan.issues.len(), 1);
    assert_eq!(plan.actions.len(), 2);
}

#[test]
fn remove_resolved_matching_glob_removes_matching_winners() {
    let dir = TempDir::new("vfsloose_remove_glob");
    let tex = dir.write("foo.dds", b"a");
    let mesh = dir.write("foo.nif", b"b");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("textures/foo.dds", &tex);
    vfs.set_winner_loose_file("textures/nested/bar.dds", &tex);
    vfs.set_winner_loose_file("meshes/foo.nif", &mesh);

    let removed = vfs.remove_resolved_matching_glob("textures/**");

    assert_eq!(removed.len(), 2);
    assert!(!vfs.contains(Path::new("textures/nested/bar.dds")));
    assert!(vfs.contains(Path::new("meshes/foo.nif")));
}

#[test]
fn remove_resolved_file_does_not_reveal_lower_priority_provider() {
    let low = TempDir::new("vfsloose_remove_winner_low");
    let high = TempDir::new("vfsloose_remove_winner_high");
    low.write("shared.txt", b"low");
    high.write("shared.txt", b"high");
    let mut vfs = VFS::from_directories([low.path(), high.path()], None);

    let removed = vfs.remove_resolved_file("shared.txt");

    assert!(removed.is_some());
    assert!(vfs.get_file("shared.txt").is_none());
}

#[test]
fn remove_winner_reveals_lower_priority_provider() {
    let low = TempDir::new("vfsloose_reveal_low");
    let high = TempDir::new("vfsloose_reveal_high");
    let low_file = low.write("textures/foo.dds", b"low");
    let high_file = high.write("textures/foo.dds", b"high");
    let mut vfs = VFS::from_directories([low.path(), high.path()], None);

    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), high_file);
    let removed = vfs
        .remove_winner("Textures\\Foo.dds")
        .expect("winner should be removed");

    assert_eq!(removed.file.path(), high_file);
    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), low_file);
}

#[test]
fn remove_source_reveals_remaining_source() {
    let low = TempDir::new("vfsloose_remove_source_low");
    let high = TempDir::new("vfsloose_remove_source_high");
    let low_file = low.write("shared.txt", b"low");
    high.write("shared.txt", b"high");
    high.write("only_high.txt", b"high");
    let mut vfs = VFS::from_directories([low.path(), high.path()], None);

    let removed = vfs.remove_source(high.path());

    assert_eq!(removed.len(), 2);
    assert_eq!(vfs.get_file("shared.txt").unwrap().path(), low_file);
    assert!(vfs.get_file("only_high.txt").is_none());
}

#[test]
fn remove_middle_provider_preserves_current_winner() {
    let low = TempDir::new("vfsloose_middle_low");
    let middle = TempDir::new("vfsloose_middle_mid");
    let high = TempDir::new("vfsloose_middle_high");
    low.write("shared.txt", b"low");
    middle.write("shared.txt", b"middle");
    let high_file = high.write("shared.txt", b"high");
    let mut vfs = VFS::from_directories([low.path(), middle.path(), high.path()], None);

    let removed = vfs.remove_provider("shared.txt", middle.path());

    assert_eq!(removed.len(), 1);
    assert_eq!(vfs.get_file("shared.txt").unwrap().path(), high_file);
    assert_eq!(vfs.providers_for("shared.txt").unwrap().count(), 2);
}

#[test]
fn push_provider_rejects_unsafe_keys() {
    let data = TempDir::new("vfsloose_unsafe_provider");
    let file = data.write("source.txt", b"data");
    let mut vfs = VFS::new();

    let inserted = vfs.push_provider(
        "../escape.txt",
        crate::VfsProvider {
            source: crate::SourceMeta {
                path: data.path().to_path_buf(),
                kind: crate::SourceKind::LooseDir,
            },
            file: crate::VfsFile::from(file),
        },
    );

    assert!(!inserted);
    assert!(vfs.providers_for("../escape.txt").is_none());
    assert_eq!(vfs.iter().count(), 0);
}

#[test]
fn remove_resolved_matching_glob_normalizes_pattern() {
    let dir = TempDir::new("vfsloose_remove_glob_normalized");
    let tex = dir.write("foo.dds", b"a");
    let mut vfs = VFS::new();
    vfs.set_winner_loose_file("textures/foo.dds", &tex);

    let removed = vfs.remove_resolved_matching_glob("Textures\\**");

    assert_eq!(removed.len(), 1);
    assert!(!vfs.contains(Path::new("textures/foo.dds")));
}

#[test]
fn from_multiple_directories_unique_files_all_present() {
    let dir1 = TempDir::new("vfsloose_multi1");
    let dir2 = TempDir::new("vfsloose_multi2");
    dir1.write("only_in_1.txt", b"1");
    dir2.write("only_in_2.txt", b"2");

    let vfs = VFS::from_directories(vec![dir1.path(), dir2.path()], None);

    assert!(vfs.get_file("only_in_1.txt").is_some());
    assert!(vfs.get_file("only_in_2.txt").is_some());
}

#[test]
fn from_directories_recurses_deeply() {
    let dir = TempDir::new("vfsloose_deep");
    dir.write("a/b/c/d/deep.txt", b"deep");

    let vfs = VFS::from_directories(vec![dir.path()], None);
    assert!(vfs.get_file("a/b/c/d/deep.txt").is_some());
}
