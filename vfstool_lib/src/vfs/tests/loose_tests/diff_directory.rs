use super::*;

// ---- diff_directory ----

#[test]
fn diff_empty_dir_against_populated_vfs_yields_no_results() {
    let vfs_dir = TempDir::new("vfsdiff_base");
    vfs_dir.write("textures/foo.dds", b"");
    let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

    let empty = TempDir::new("vfsdiff_empty_mod");
    let diff = vfs.diff_directory(empty.path());
    assert!(diff.conflicts.is_empty());
    assert!(diff.additions.is_empty());
}

#[test]
fn diff_dir_with_only_new_files_yields_only_additions() {
    let vfs_dir = TempDir::new("vfsdiff_newfiles_base");
    vfs_dir.write("textures/vanilla.dds", b"");
    let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

    let mod_dir = TempDir::new("vfsdiff_newfiles_mod");
    let new1 = mod_dir.write("meshes/new_mesh.nif", b"");
    let new2 = mod_dir.write("textures/new_tex.dds", b"");

    let diff = vfs.diff_directory(mod_dir.path());
    assert!(diff.conflicts.is_empty());
    assert_eq!(diff.additions.len(), 2);

    let addition_paths: Vec<_> = diff
        .additions
        .iter()
        .map(|(_, f)| f.path().to_path_buf())
        .collect();
    assert!(addition_paths.contains(&new1));
    assert!(addition_paths.contains(&new2));
}

#[test]
fn diff_dir_with_only_conflicting_files_yields_only_conflicts() {
    let vfs_dir = TempDir::new("vfsdiff_conflicts_base");
    let existing = vfs_dir.write("textures/shared.dds", b"original");
    let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

    let mod_dir = TempDir::new("vfsdiff_conflicts_mod");
    let replacement = mod_dir.write("textures/shared.dds", b"replacement");

    let diff = vfs.diff_directory(mod_dir.path());
    assert!(diff.additions.is_empty());
    assert_eq!(diff.conflicts.len(), 1);

    let (key, incoming, displaced) = &diff.conflicts[0];
    assert_eq!(key.as_os_str(), "textures/shared.dds");
    assert_eq!(incoming.path(), replacement);
    assert_eq!(displaced.path(), existing);
}

#[test]
fn diff_dir_mixed_yields_correct_split() {
    let vfs_dir = TempDir::new("vfsdiff_mixed_base");
    let existing = vfs_dir.write("textures/shared.dds", b"original");
    vfs_dir.write("textures/untouched.dds", b"");
    let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

    let mod_dir = TempDir::new("vfsdiff_mixed_mod");
    let replacement = mod_dir.write("textures/shared.dds", b"mod_version");
    let addition = mod_dir.write("meshes/new.nif", b"");

    let diff = vfs.diff_directory(mod_dir.path());

    assert_eq!(diff.conflicts.len(), 1);
    assert_eq!(diff.additions.len(), 1);

    let (_, incoming, displaced) = &diff.conflicts[0];
    assert_eq!(incoming.path(), replacement);
    assert_eq!(displaced.path(), existing);

    assert_eq!(diff.additions[0].1.path(), addition);
}

/// A file whose case/separators differ between dir and VFS must still be
/// detected as a conflict — normalization applies to both sides.
#[test]
fn diff_dir_conflict_detected_across_case_normalization() {
    let vfs_dir = TempDir::new("vfsdiff_case_base");
    vfs_dir.write("textures/foo.dds", b"lowercase");
    let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

    let mod_dir = TempDir::new("vfsdiff_case_mod");
    // Mod stores the file with mixed case — same VFS key after normalization
    let replacement = mod_dir.write("Textures/Foo.DDS", b"mod_version");

    let diff = vfs.diff_directory(mod_dir.path());
    assert_eq!(
        diff.conflicts.len(),
        1,
        "case variant should be detected as conflict"
    );
    assert_eq!(diff.conflicts[0].1.path(), replacement);
}

/// Deeply nested files and subdirectories are all classified correctly.
#[test]
fn diff_dir_handles_deep_nesting() {
    let vfs_dir = TempDir::new("vfsdiff_deep_base");
    vfs_dir.write("a/b/c/d/deep.txt", b"");
    let vfs = VFS::from_directories(vec![vfs_dir.path()], None);

    let mod_dir = TempDir::new("vfsdiff_deep_mod");
    let replacement = mod_dir.write("a/b/c/d/deep.txt", b"mod");
    let addition = mod_dir.write("a/b/c/d/new.txt", b"new");

    let diff = vfs.diff_directory(mod_dir.path());
    assert_eq!(diff.conflicts.len(), 1);
    assert_eq!(diff.additions.len(), 1);
    assert_eq!(diff.conflicts[0].1.path(), replacement);
    assert_eq!(diff.additions[0].1.path(), addition);
}

/// `diff_directory` against an empty VFS: everything is an addition.
#[test]
fn diff_dir_against_empty_vfs_yields_all_additions() {
    let empty_base = TempDir::new("vfsdiff_emptyvfs_base");
    let vfs = VFS::from_directories(vec![empty_base.path()], None);

    let mod_dir = TempDir::new("vfsdiff_emptyvfs_mod");
    mod_dir.write("a.txt", b"");
    mod_dir.write("b.txt", b"");

    let diff = vfs.diff_directory(mod_dir.path());
    assert!(diff.conflicts.is_empty());
    assert_eq!(diff.additions.len(), 2);
}
