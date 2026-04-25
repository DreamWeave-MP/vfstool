use super::*;

#[test]
fn semantic_conflicts_identical_and_different() {
    let low = TempDir::new("analysis_semantic_low");
    let high = TempDir::new("analysis_semantic_high");

    low.write("textures/same.dds", b"identical");
    high.write("textures/same.dds", b"identical");
    low.write("textures/diff.dds", b"aaa");
    high.write("textures/diff.dds", b"bbb");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let report = index
        .semantic_conflicts(&vfs)
        .expect("semantic conflict report should succeed");

    let same = report
        .entries
        .iter()
        .find(|e| e.key == Path::new("textures/same.dds"))
        .expect("expected same.dds entry");
    assert!(same.all_identical);
    assert_eq!(same.distinct_versions, 1);

    let diff = report
        .entries
        .iter()
        .find(|e| e.key == Path::new("textures/diff.dds"))
        .expect("expected diff.dds entry");
    assert!(!diff.all_identical);
    assert_eq!(diff.distinct_versions, 2);
}
#[test]
fn semantic_conflicts_reads_mixed_case_loose_provider_paths() {
    let low = TempDir::new("analysis_semantic_mixed_low");
    let high = TempDir::new("analysis_semantic_mixed_high");
    low.write("Textures/Foo.DDS", b"low");
    high.write("textures/foo.dds", b"high");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let report = index.semantic_conflicts(&vfs).expect("semantic report");
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.key == Path::new("textures/foo.dds"))
        .expect("mixed-case conflict should be reported");

    assert_eq!(entry.distinct_versions, 2);
    assert!(
        entry
            .providers
            .iter()
            .all(|provider| provider.hash_blake3.is_some())
    );
}

#[test]
fn semantic_conflict_omits_key_without_actual_vfs_winner() {
    let low = TempDir::new("analysis_semantic_no_winner_hash_low");
    low.write("shared.txt", b"same");
    let index = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: low.path().to_path_buf(),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("shared.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("missing.bsa"),
                kind: SourceKind::Archive,
            },
            vec![PathBuf::from("shared.txt")],
        ),
    ]);
    let vfs = VFS::new();

    let entry = index
        .semantic_conflict_for_key_no_cache(&vfs, Path::new("shared.txt"), SemanticOpts::default())
        .expect("semantic conflict should build");

    assert!(entry.is_none());
}

#[test]
#[cfg(feature = "zip")]
fn semantic_conflicts_all_providers_hashes_zip_archives() {
    use std::io::Write as _;

    fn write_zip(path: &Path, entry: &str, data: &[u8]) {
        let file = fs::File::create(path).expect("zip file should be created");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file(entry, options)
            .expect("entry should start");
        writer.write_all(data).expect("entry should be written");
        writer.finish().expect("zip should finish");
    }

    let data = TempDir::new("analysis_semantic_zip_all_providers");
    write_zip(&data.path().join("low.zip"), "Textures/Foo.DDS", b"low");
    write_zip(&data.path().join("high.zip"), "textures/foo.dds", b"high");

    let (vfs, index) =
        VFS::from_directories_with_layer_index([data.path()], Some(vec!["low.zip", "high.zip"]));
    let report = index
        .semantic_conflicts_with_opts(
            &vfs,
            SemanticOpts {
                archive_hash_mode: ArchiveHashMode::AllProviders,
                include_semantic_deltas: false,
            },
        )
        .expect("semantic report should build");
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.key == Path::new("textures/foo.dds"))
        .expect("archive conflict should be reported");

    assert_eq!(entry.distinct_versions, 2);
    assert!(
        entry
            .providers
            .iter()
            .all(|provider| provider.hash_blake3.is_some())
    );
}
#[test]
fn semantic_conflicts_enrich_adds_asset_class_and_delta() {
    let low = TempDir::new("analysis_semantic_enrich_low");
    let high = TempDir::new("analysis_semantic_enrich_high");

    low.write("config/example.ini", b"[sec]\na=1\nb=2\n");
    high.write("config/example.ini", b"# comment\n[sec]\nb=2\na=1\n");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let report = index
        .semantic_conflicts_with_opts(
            &vfs,
            SemanticOpts {
                include_semantic_deltas: true,
                ..SemanticOpts::default()
            },
        )
        .expect("semantic enrich should succeed");

    let entry = report
        .entries
        .iter()
        .find(|entry| entry.key == Path::new("config/example.ini"))
        .expect("expected example.ini conflict entry");

    assert_eq!(entry.asset_class, AssetClass::Ini);
    assert!(
        entry
            .providers
            .iter()
            .all(|provider| provider.semantic_delta_to_winner.is_some())
    );
    assert!(entry.providers.iter().any(|provider| {
        provider.semantic_delta_to_winner == Some(SemanticDelta::CosmeticOnly)
    }));
}

#[test]
fn semantic_conflicts_are_deterministic_across_runs() {
    let low = TempDir::new("analysis_semantic_deterministic_low");
    let high = TempDir::new("analysis_semantic_deterministic_high");
    low.write("scripts/a.lua", b"print('a')\n");
    high.write("scripts/a.lua", b"print('b')\n");
    low.write("config/example.ini", b"[sec]\na=1\n");
    high.write("config/example.ini", b"[sec]\na=2\n");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let opts = SemanticOpts {
        include_semantic_deltas: true,
        ..SemanticOpts::default()
    };
    let first = index
        .semantic_conflicts_with_opts(&vfs, opts)
        .expect("first semantic report should succeed");
    let second = index
        .semantic_conflicts_with_opts(&vfs, opts)
        .expect("second semantic report should succeed");

    let first_rows = first
        .entries
        .iter()
        .map(|entry| {
            (
                entry.key.clone(),
                entry.winner.path.clone(),
                entry.winner.kind,
                entry.asset_class,
                entry.all_identical,
                entry.distinct_versions,
                entry
                    .providers
                    .iter()
                    .map(|provider| {
                        (
                            provider.source.path.clone(),
                            provider.source.kind,
                            provider.relation,
                            provider.hash_blake3.clone(),
                            provider.size,
                            provider.semantic_delta_to_winner.clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let second_rows = second
        .entries
        .iter()
        .map(|entry| {
            (
                entry.key.clone(),
                entry.winner.path.clone(),
                entry.winner.kind,
                entry.asset_class,
                entry.all_identical,
                entry.distinct_versions,
                entry
                    .providers
                    .iter()
                    .map(|provider| {
                        (
                            provider.source.path.clone(),
                            provider.source.kind,
                            provider.relation,
                            provider.hash_blake3.clone(),
                            provider.size,
                            provider.semantic_delta_to_winner.clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(first_rows, second_rows);
}
