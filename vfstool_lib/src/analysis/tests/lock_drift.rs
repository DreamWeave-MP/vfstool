use super::*;

#[test]
fn lock_manifest_is_sorted() {
    let data = TempDir::new("analysis_lock_sorted");
    data.write("textures/z.dds", b"z");
    data.write("textures/a.dds", b"a");

    let (vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);
    let lock = index
        .lock_manifest(&vfs)
        .expect("lock manifest should succeed");
    assert_eq!(lock.schema_version, 1);
    assert_eq!(lock.entries[0].key, PathBuf::from("textures/a.dds"));
    assert_eq!(lock.entries[1].key, PathBuf::from("textures/z.dds"));
}

#[test]
fn lock_manifest_hashes_mixed_case_loose_winner_path() {
    let data = TempDir::new("analysis_lock_mixed_case");
    data.write("Textures/Foo.DDS", b"mixed");

    let (vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);
    let lock = index.lock_manifest(&vfs).expect("lock should build");

    assert_eq!(lock.entries[0].key, PathBuf::from("textures/foo.dds"));
    assert_eq!(lock.entries[0].winner_size, Some(5));
    assert!(lock.entries[0].winner_hash_blake3.is_some());
}

#[test]
fn lock_manifest_hashes_actual_same_source_winner_occurrence() {
    let data = TempDir::new("analysis_lock_same_source_occurrence");
    data.write("Textures/Foo.DDS", b"long losing content");
    data.write("textures/foo.dds", b"win");

    let (vfs, index) = VFS::from_directories_with_layer_index([data.path()], None);
    let winner_len = fs::read(vfs.get_file("textures/foo.dds").unwrap().path())
        .unwrap()
        .len() as u64;
    let lock = index.lock_manifest(&vfs).expect("lock should build");

    assert_eq!(lock.entries[0].key, PathBuf::from("textures/foo.dds"));
    assert_eq!(lock.entries[0].winner_size, Some(winner_len));
}

#[test]
fn lock_manifest_uses_actual_vfs_winner_presence() {
    let low = TempDir::new("analysis_lock_removed_low");
    let high = TempDir::new("analysis_lock_removed_high");
    low.write("shared.txt", b"low");
    high.write("shared.txt", b"high");

    let (mut vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    vfs.remove_resolved_file("shared.txt");

    let lock = index.lock_manifest(&vfs).expect("lock should build");
    assert!(lock.entries.is_empty());
}

#[test]
fn lock_manifest_is_deterministic_across_runs() {
    let low = TempDir::new("analysis_lock_deterministic_low");
    let high = TempDir::new("analysis_lock_deterministic_high");
    low.write("textures/a.dds", b"aaa");
    high.write("textures/a.dds", b"bbb");
    low.write("meshes/m.nif", b"m");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let first = index
        .lock_manifest(&vfs)
        .expect("first lock build should succeed");
    let second = index
        .lock_manifest(&vfs)
        .expect("second lock build should succeed");

    let first_rows = first
        .entries
        .iter()
        .map(|entry| {
            (
                entry.key.clone(),
                entry.winner_source.clone(),
                entry.winner_kind,
                entry.winner_hash_blake3.clone(),
                entry.winner_size,
                entry.provider_count,
            )
        })
        .collect::<Vec<_>>();
    let second_rows = second
        .entries
        .iter()
        .map(|entry| {
            (
                entry.key.clone(),
                entry.winner_source.clone(),
                entry.winner_kind,
                entry.winner_hash_blake3.clone(),
                entry.winner_size,
                entry.provider_count,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(first_rows, second_rows);
}
#[test]
fn drift_detects_source_and_hash_changes() {
    let low = TempDir::new("analysis_drift_low");
    let high = TempDir::new("analysis_drift_high");
    low.write("textures/a.dds", b"aaa");
    high.write("textures/a.dds", b"bbb");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let mut lock = index
        .lock_manifest(&vfs)
        .expect("lock build should succeed");

    lock.entries[0].winner_source = low.path().to_path_buf();
    lock.entries[0].winner_hash_blake3 = Some("00".repeat(32));

    let drift = index
        .diff_against_lock(&vfs, &lock)
        .expect("drift diff should succeed");

    assert!(
        drift
            .entries
            .iter()
            .any(|entry| entry.kind == DriftKind::WinnerSourceChanged)
    );
    assert!(
        drift
            .entries
            .iter()
            .any(|entry| entry.kind == DriftKind::WinnerHashChanged)
    );
}
