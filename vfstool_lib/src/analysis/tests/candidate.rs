use super::*;

#[test]
fn candidate_plan_reports_additions_and_conflicts() {
    let low = TempDir::new("analysis_plan_low");
    let high = TempDir::new("analysis_plan_high");
    let candidate = TempDir::new("analysis_plan_candidate");

    low.write("textures/a.dds", b"low");
    high.write("textures/a.dds", b"high");
    high.write("textures/b.dds", b"b");

    candidate.write("textures/a.dds", b"candidate-new");
    candidate.write("textures/c.dds", b"c");

    let (vfs, index) = VFS::from_directories_with_layer_index([low.path(), high.path()], None);
    let plan = index
        .plan_candidate_directory(&vfs, candidate.path(), CandidatePlanOpts::default())
        .expect("candidate plan should succeed");

    assert_eq!(plan.summary.additions, 1);
    assert_eq!(plan.summary.conflicts, 1);
    assert_eq!(plan.summary.displaced_winners, 1);
    assert_eq!(plan.additions[0], PathBuf::from("textures/c.dds"));
    assert_eq!(plan.conflicts[0].key, PathBuf::from("textures/a.dds"));
    assert_eq!(plan.conflicts[0].semantic_differs, Some(true));
}

#[test]
fn candidate_plan_semantic_can_be_disabled() {
    let base = TempDir::new("analysis_plan_semantic_base");
    let candidate = TempDir::new("analysis_plan_semantic_candidate");
    base.write("textures/a.dds", b"same");
    candidate.write("textures/a.dds", b"same");

    let (vfs, index) = VFS::from_directories_with_layer_index([base.path()], None);
    let plan = index
        .plan_candidate_directory(
            &vfs,
            candidate.path(),
            CandidatePlanOpts {
                include_semantic: false,
            },
        )
        .expect("candidate plan should succeed");

    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].semantic_differs, None);
}

#[test]
fn candidate_plan_deduplicates_normalized_candidate_keys() {
    let base = TempDir::new("analysis_plan_dedupe_base");
    let candidate = TempDir::new("analysis_plan_dedupe_candidate");
    base.write("textures/a.dds", b"base");
    candidate.write("textures/a.dds", b"candidate-lower");
    candidate.write("Textures/A.dds", b"candidate-upper");
    candidate.write("meshes/b.nif", b"candidate-lower");
    candidate.write("Meshes/B.nif", b"candidate-upper");

    let (vfs, index) = VFS::from_directories_with_layer_index([base.path()], None);
    let plan = index
        .plan_candidate_directory(&vfs, candidate.path(), CandidatePlanOpts::default())
        .expect("candidate plan should succeed");

    assert_eq!(plan.summary.additions, 1);
    assert_eq!(plan.summary.conflicts, 1);
    assert_eq!(plan.summary.displaced_winners, 1);
    assert_eq!(plan.additions, vec![PathBuf::from("meshes/b.nif")]);
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].key, PathBuf::from("textures/a.dds"));
}
