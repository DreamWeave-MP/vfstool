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
fn layer_index_deduplicates_keys_within_one_source() {
    let index = LayerIndex::from_file_lists(vec![(
        SourceMeta {
            path: PathBuf::from("/one"),
            kind: SourceKind::LooseDir,
        },
        vec![PathBuf::from("shared.txt"), PathBuf::from("SHARED.TXT")],
    )]);

    assert_eq!(index.sources_containing(Path::new("shared.txt")), &[0]);
}

#[test]
fn layer_index_includes_unique_provider_keys() {
    let data = TempDir::new("analysis_unique_provider");
    data.write("unique.txt", b"unique");
    let (_vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);

    assert_eq!(index.sources_containing(Path::new("unique.txt")), &[0]);
}
