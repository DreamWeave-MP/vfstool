use super::*;

#[test]
fn simulate_swap_changes_winner() {
    let low = TempDir::new("analysis_sim_low");
    let high = TempDir::new("analysis_sim_high");
    low.write("textures/a.dds", b"low");
    high.write("textures/a.dds", b"high");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let delta = index
        .simulate(
            &vfs,
            ReorderOp::Swap(low.path().to_path_buf(), high.path().to_path_buf()),
        )
        .expect("simulate should succeed");

    assert_eq!(delta.changed_winners, 1);
}

#[test]
fn simulate_move_before_changes_winner() {
    let a = TempDir::new("analysis_sim_move_before_a");
    let b = TempDir::new("analysis_sim_move_before_b");
    let c = TempDir::new("analysis_sim_move_before_c");
    a.write("textures/a.dds", b"a");
    b.write("textures/a.dds", b"b");
    c.write("textures/a.dds", b"c");

    let (vfs, index) = VFS::from_directories_with_layer_index([a.path(), b.path(), c.path()], None);
    let delta = index
        .simulate(
            &vfs,
            ReorderOp::MoveBefore {
                source: c.path().to_path_buf(),
                before: a.path().to_path_buf(),
            },
        )
        .expect("simulate move-before should succeed");

    assert_eq!(delta.changed_winners, 1);
}

#[test]
fn simulate_move_after_changes_winner() {
    let a = TempDir::new("analysis_sim_move_after_a");
    let b = TempDir::new("analysis_sim_move_after_b");
    a.write("textures/a.dds", b"a");
    b.write("textures/a.dds", b"b");

    let (vfs, index) = VFS::from_directories_with_layer_index([a.path(), b.path()], None);
    let delta = index
        .simulate(
            &vfs,
            ReorderOp::MoveAfter {
                source: a.path().to_path_buf(),
                after: b.path().to_path_buf(),
            },
        )
        .expect("simulate move-after should succeed");

    assert_eq!(delta.changed_winners, 1);
}

#[test]
fn simulate_full_order_rejects_duplicate_sources() {
    let a = TempDir::new("analysis_sim_full_dup_a");
    let b = TempDir::new("analysis_sim_full_dup_b");
    a.write("textures/a.dds", b"a");
    b.write("textures/a.dds", b"b");

    let (vfs, index) = VFS::from_directories_with_layer_index([a.path(), b.path()], None);
    let err = index
        .simulate(
            &vfs,
            ReorderOp::FullOrder(vec![a.path().to_path_buf(), a.path().to_path_buf()]),
        )
        .expect_err("duplicate full-order should error");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn simulate_with_buckets_reports_counts() {
    let low = TempDir::new("analysis_sim_bucket_low");
    let high = TempDir::new("analysis_sim_bucket_high");
    low.write("textures/a.dds", b"low");
    high.write("textures/a.dds", b"high");
    low.write("meshes/a.nif", b"low");
    high.write("meshes/a.nif", b"high");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let opts = SimOpts {
        sample_limit: 10,
        impact_buckets: vec!["textures/**".into(), "meshes/**".into()],
    };
    let delta = index
        .simulate_with_opts(
            &vfs,
            ReorderOp::Swap(low.path().to_path_buf(), high.path().to_path_buf()),
            &opts,
        )
        .expect("simulate with buckets should succeed");

    assert_eq!(delta.by_bucket.len(), 2);
    assert_eq!(delta.by_bucket[0].changed_winners, 1);
    assert_eq!(delta.by_bucket[1].changed_winners, 1);
}

#[test]
#[cfg(feature = "zip")]
fn simulate_full_order_preserves_manual_archive_winner() {
    use std::io::Write as _;

    let loose = TempDir::new("analysis_sim_manual_archive_loose");
    loose.write("textures/a.dds", b"loose");
    let archive_root = TempDir::new("analysis_sim_manual_archive_root");
    let archive_path = archive_root.path().join("override.zip");
    let file = fs::File::create(&archive_path).expect("zip should be created");
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("textures/a.dds", zip::write::SimpleFileOptions::default())
        .expect("zip entry should start");
    writer
        .write_all(b"archive")
        .expect("zip entry should write");
    writer.finish().expect("zip should finish");

    let mut vfs = VFS::from_directories([loose.path()], None);
    assert!(vfs.push_archive(&archive_path));
    let index = vfs.layer_index().clone();

    let delta = index
        .simulate(
            &vfs,
            ReorderOp::FullOrder(vec![loose.path().to_path_buf(), archive_path]),
        )
        .expect("simulation should succeed");

    assert_eq!(delta.changed_winners, 0);
    assert_eq!(delta.by_source_gain_loss[1].wins_after, 1);
}

#[test]
fn simulate_impact_scores_with_profile() {
    let low = TempDir::new("analysis_impact_low");
    let high = TempDir::new("analysis_impact_high");
    low.write("scripts/x.lua", b"print('a')\n");
    high.write("scripts/x.lua", b"print('b')\n");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let opts = SimOpts {
        sample_limit: 50,
        impact_buckets: vec!["scripts/**".into()],
    };
    let profile = ImpactProfile {
        heuristics: vec![ImpactHeuristic {
            name: "scripts-change".into(),
            path_glob: "scripts/**".into(),
            weight: 3.0,
            condition: HeuristicCondition::WinnerChanged,
        }],
    };

    let report = index
        .simulate_impact(
            &vfs,
            ReorderOp::Swap(low.path().to_path_buf(), high.path().to_path_buf()),
            &opts,
            &profile,
        )
        .expect("simulate impact should succeed");

    assert!(report.overall_score > 0.0);
    assert!(!report.top_risky_changes.is_empty());
    assert_eq!(report.by_bucket.len(), 1);
}

#[test]
fn simulate_impact_semantic_score_uses_before_after_winners_only() {
    let low = TempDir::new("analysis_impact_semantic_low");
    let mid = TempDir::new("analysis_impact_semantic_mid");
    let high = TempDir::new("analysis_impact_semantic_high");
    low.write("config/test.json", br#"{"value":2}"#);
    mid.write("config/test.json", br#"{"value":1}"#);
    high.write(
        "config/test.json",
        br#"{
  "value": 1
}"#,
    );

    let (vfs, index) =
        VFS::from_directories_with_layer_index([low.path(), mid.path(), high.path()], None);
    let opts = SimOpts {
        sample_limit: 50,
        impact_buckets: vec!["config/**".into()],
    };
    let profile = ImpactProfile {
        heuristics: vec![ImpactHeuristic {
            name: "semantic-change".into(),
            path_glob: "config/**".into(),
            weight: 5.0,
            condition: HeuristicCondition::WinnerChangedAndSemanticBehaviorChanging,
        }],
    };

    let report = index
        .simulate_impact(
            &vfs,
            ReorderOp::Swap(mid.path().to_path_buf(), high.path().to_path_buf()),
            &opts,
            &profile,
        )
        .expect("simulate impact should succeed");

    assert!(report.overall_score.abs() <= f32::EPSILON);
    assert!(report.top_risky_changes.is_empty());
}
