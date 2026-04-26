// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{fs, path::PathBuf};

use vfstool_lib::{CollapseOptions, MaterializationAction, SourceKind, VFS};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "vfstool_provider_api_{name}_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn provider_reports_preserve_priority_and_explain_winner() {
    let root = temp_root("priority");
    let low = root.join("low");
    let high = root.join("high");
    fs::create_dir_all(low.join("textures")).unwrap();
    fs::create_dir_all(high.join("Textures")).unwrap();
    fs::write(low.join("textures/foo.dds"), b"low").unwrap();
    fs::write(high.join("Textures/Foo.DDS"), b"high").unwrap();

    let vfs = VFS::from_directories([&low, &high], None);
    let providers = vfs.providers_for("textures/foo.dds");
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].source.path, low);
    assert_eq!(providers[1].source.path, high);
    assert_eq!(providers[0].source.kind, SourceKind::LooseDir);

    let explanation = vfs.explain("Textures/Foo.DDS").unwrap();
    assert_eq!(explanation.winner.source.path, high);
    assert_eq!(explanation.overridden.len(), 1);
    assert_eq!(explanation.overridden[0].source.path, low);

    let duplicates = vfs.duplicates();
    assert_eq!(duplicates.entries.len(), 1);
    assert_eq!(duplicates.entries[0].winner_index, 1);

    let collisions = vfs.case_collisions();
    assert_eq!(collisions.collisions.len(), 1);
    assert_eq!(collisions.collisions[0].providers.len(), 2);

    let contributions = vfs.source_contributions();
    assert_eq!(contributions.sources.len(), 2);
    assert_eq!(contributions.sources[0].overridden_files, 1);
    assert_eq!(contributions.sources[1].winning_files, 1);

    let plan = vfs.materialization_plan(
        root.join("out"),
        &CollapseOptions {
            allow_copying: true,
            extract_archives: false,
            use_symlinks: false,
        },
    );
    assert!(matches!(
        plan.actions.as_slice(),
        [MaterializationAction::Copy { .. }]
    ));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn validate_reports_cross_source_file_directory_conflicts() {
    let root = temp_root("validate");
    let file_source = root.join("file_source");
    let child_source = root.join("child_source");
    fs::create_dir_all(&file_source).unwrap();
    fs::create_dir_all(child_source.join("foo")).unwrap();
    fs::write(file_source.join("foo"), b"file").unwrap();
    fs::write(child_source.join("foo/bar.txt"), b"child").unwrap();

    let vfs = VFS::from_directories([&file_source, &child_source], None);
    assert_eq!(vfs.validate().issues.len(), 1);
    assert_eq!(
        vfs.materialization_plan(
            root.join("out"),
            &CollapseOptions {
                allow_copying: false,
                extract_archives: false,
                use_symlinks: false
            },
        )
        .issues
        .len(),
        1
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn mutators_keep_provider_reports_in_sync() {
    let mut vfs = VFS::new();
    let path = PathBuf::from("/tmp/vfstool-provider-api/source/foo.txt");
    assert!(vfs.insert_loose_file("Foo.TXT", &path).is_none());
    assert_eq!(vfs.providers_for("foo.txt").len(), 1);
    assert!(vfs.explain("foo.txt").is_some());

    assert!(vfs.remove_file("foo.txt").is_some());
    assert!(vfs.providers_for("foo.txt").is_empty());
    assert!(vfs.explain("foo.txt").is_none());
}
