use super::*;

#[test]
fn provenance_chain_respects_load_order() {
    let low = TempDir::new("analysis_provenance_low");
    let high = TempDir::new("analysis_provenance_high");
    low.write("textures/a.dds", b"low");
    high.write("textures/a.dds", b"high");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let chain = index
        .provenance(&vfs, Path::new("textures/a.dds"), false)
        .expect("provenance should succeed")
        .expect("path should exist");

    assert_eq!(chain.providers.len(), 2);
    assert_eq!(chain.providers[0].source.path, low.path());
    assert_eq!(chain.providers[1].source.path, high.path());
    assert_eq!(chain.winner.path, high.path());
}

#[test]
fn layer_index_preserves_provider_occurrences_within_one_source() {
    let index = LayerIndex::from_file_lists(vec![(
        SourceMeta {
            path: PathBuf::from("/one"),
            kind: SourceKind::LooseDir,
        },
        vec![PathBuf::from("shared.txt"), PathBuf::from("SHARED.TXT")],
    )]);

    assert_eq!(index.sources_containing(Path::new("shared.txt")), &[0, 0]);
    let chain = index.provider_chain(Path::new("shared.txt"));
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].provider_index, 0);
    assert_eq!(chain[0].original_path, PathBuf::from("shared.txt"));
    assert_eq!(chain[1].provider_index, 1);
    assert_eq!(chain[1].original_path, PathBuf::from("SHARED.TXT"));
}

#[test]
fn layer_index_includes_unique_provider_keys() {
    let data = TempDir::new("analysis_unique_provider");
    data.write("unique.txt", b"unique");
    let (_vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);

    assert_eq!(index.sources_containing(Path::new("unique.txt")), &[0]);
}

#[test]
fn layer_index_skips_unsafe_provider_keys() {
    let index = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/one"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("../escape.txt"), PathBuf::from("safe.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/two"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("/absolute.txt")],
        ),
    ]);

    assert_eq!(index.keys(), vec![crate::NormalizedPath::from("safe.txt")]);
}

#[test]
fn source_contributions_count_middle_sources_as_overriding_and_overridden() {
    let index = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/low"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("shared.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/mid"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("shared.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/high"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("shared.txt")],
        ),
    ]);

    let contributions = index.source_contributions();
    let low = &contributions.sources[0];
    let mid = &contributions.sources[1];
    let high = &contributions.sources[2];

    assert_eq!(low.overridden_files, 1);
    assert_eq!(low.overriding_files, 0);
    assert_eq!(mid.overridden_files, 1);
    assert_eq!(mid.overriding_files, 1);
    assert_eq!(high.winning_files, 1);
    assert_eq!(high.overriding_files, 1);
}

#[test]
fn same_source_duplicate_occurrences_do_not_count_as_cross_source_overrides() {
    let index = LayerIndex::from_file_lists(vec![(
        SourceMeta {
            path: PathBuf::from("/one"),
            kind: SourceKind::LooseDir,
        },
        vec![PathBuf::from("shared.txt"), PathBuf::from("SHARED.TXT")],
    )]);

    let contributions = index.source_contributions();
    let source = &contributions.sources[0];

    assert_eq!(source.duplicate_files, 2);
    assert_eq!(source.winning_files, 2);
    assert_eq!(source.overriding_files, 0);
    assert_eq!(source.overridden_files, 0);
}

#[test]
fn vfs_layer_index_preserves_same_source_case_collisions() {
    let data = TempDir::new("analysis_layer_same_source_case_collision");
    data.write("Textures/Foo.DDS", b"upper");
    data.write("textures/foo.dds", b"lower");

    let (vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);

    assert_eq!(vfs.provider_records_for("textures/foo.dds").len(), 2);
    let chain = index.provider_chain(Path::new("textures/foo.dds"));
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].source_index, 0);
    assert_eq!(chain[1].source_index, 0);
}
