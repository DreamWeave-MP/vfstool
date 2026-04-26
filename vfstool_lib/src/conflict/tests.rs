use super::*;
use crate::{LayerIndex, SourceKind, SourceMeta};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, data: &[u8]) {
        let target = self.0.join(rel);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, data).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn no_overlap_no_conflicts() {
    let d1 = TempDir::new("ci_nooverlap_d1");
    let d2 = TempDir::new("ci_nooverlap_d2");
    d1.write("a.txt", b"");
    d2.write("b.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

    assert!(!index.conflicts[0].has_overrides());
    assert!(!index.conflicts[0].is_overridden());
    assert!(!index.conflicts[1].has_overrides());
    assert!(!index.conflicts[1].is_overridden());
}

#[test]
fn later_dir_shows_green_earlier_dir_shows_red() {
    let d1 = TempDir::new("ci_greenred_d1");
    let d2 = TempDir::new("ci_greenred_d2");
    d1.write("shared.txt", b"");
    d2.write("shared.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

    // d1 (earlier, lower priority): overridden, not overriding
    assert!(!index.conflicts[0].has_overrides(), "d1 overrides nothing");
    assert!(index.conflicts[0].is_overridden(), "d1 is overridden by d2");

    // d2 (later, higher priority): overriding, not overridden
    assert!(index.conflicts[1].has_overrides(), "d2 overrides d1");
    assert!(!index.conflicts[1].is_overridden(), "nothing overrides d2");
}

#[test]
fn middle_dir_shows_both_arrows() {
    let d1 = TempDir::new("ci_both_d1");
    let d2 = TempDir::new("ci_both_d2");
    let d3 = TempDir::new("ci_both_d3");
    d1.write("shared.txt", b"");
    d2.write("shared.txt", b"");
    d3.write("shared.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);

    assert!(!index.conflicts[0].has_overrides());
    assert!(index.conflicts[0].is_overridden());

    // d2 is in the middle: overrides d1, overridden by d3
    assert!(
        index.conflicts[1].has_overrides(),
        "d2 should have green arrow"
    );
    assert!(
        index.conflicts[1].is_overridden(),
        "d2 should have red arrow"
    );

    assert!(index.conflicts[2].has_overrides());
    assert!(!index.conflicts[2].is_overridden());
}

#[test]
fn conflict_paths_are_normalized() {
    let d1 = TempDir::new("ci_norm_d1");
    let d2 = TempDir::new("ci_norm_d2");
    d1.write("Textures/Foo.DDS", b"");
    d2.write("textures/foo.dds", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

    // Despite different on-disk names, they normalize to the same key
    assert!(index.conflicts[1].has_overrides());
    assert!(index.conflicts[0].is_overridden());

    let key = PathBuf::from("textures/foo.dds");
    assert!(index.conflicts[1].overrides.contains(&key));
    assert!(index.conflicts[0].overridden_by.contains(&key));
}

#[test]
fn unique_files_not_in_conflict_sets() {
    let d1 = TempDir::new("ci_unique_d1");
    let d2 = TempDir::new("ci_unique_d2");
    d1.write("shared.txt", b"");
    d1.write("only_in_d1.txt", b"");
    d2.write("shared.txt", b"");
    d2.write("only_in_d2.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

    let unique1 = PathBuf::from("only_in_d1.txt");
    let unique2 = PathBuf::from("only_in_d2.txt");
    assert!(!index.conflicts[0].overridden_by.contains(&unique1));
    assert!(!index.conflicts[1].overrides.contains(&unique2));
}

#[test]
fn duplicate_keys_within_one_source_do_not_self_conflict() {
    let index = ConflictIndex::from_file_lists(vec![
        (
            PathBuf::from("/one"),
            vec![PathBuf::from("shared.txt"), PathBuf::from("shared.txt")],
        ),
        (PathBuf::from("/two"), vec![PathBuf::from("unique.txt")]),
    ]);

    assert!(index.sources_containing(Path::new("shared.txt")).is_empty());
    assert!(index.conflicts[0].overrides.is_empty());
    assert!(index.conflicts[0].overridden_by.is_empty());
}

#[test]
fn from_file_lists_normalizes_caller_supplied_paths() {
    let index = ConflictIndex::from_file_lists(vec![
        (
            PathBuf::from("/one"),
            vec![PathBuf::from("Textures/Foo.DDS")],
        ),
        (
            PathBuf::from("/two"),
            vec![PathBuf::from("textures\\foo.dds")],
        ),
    ]);

    assert_eq!(
        index.sources_containing(Path::new("textures/foo.dds")),
        &[0, 1]
    );
    assert!(index.conflicts[1].has_overrides());
    assert!(index.conflicts[0].is_overridden());
}

#[test]
fn sources_containing_returns_indices_in_order() {
    let d1 = TempDir::new("ci_dircontaining_d1");
    let d2 = TempDir::new("ci_dircontaining_d2");
    let d3 = TempDir::new("ci_dircontaining_d3");
    d1.write("shared.txt", b"");
    d2.write("shared.txt", b"");
    d3.write("shared.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);

    let indices = index.sources_containing(Path::new("shared.txt"));
    assert_eq!(indices, &[0, 1, 2]);
}

#[test]
fn displaced_by_returns_next_lower_priority_dir() {
    let d1 = TempDir::new("ci_displaced_d1");
    let d2 = TempDir::new("ci_displaced_d2");
    let d3 = TempDir::new("ci_displaced_d3");
    d1.write("shared.txt", b"");
    d2.write("shared.txt", b"");
    d3.write("shared.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);
    let path = Path::new("shared.txt");

    // d3 (index 2) directly displaces d2 (index 1)
    assert_eq!(index.displaced_by(2, path), Some(1));
    // d2 (index 1) directly displaces d1 (index 0)
    assert_eq!(index.displaced_by(1, path), Some(0));
    // d1 (index 0) displaces nothing
    assert_eq!(index.displaced_by(0, path), None);
}

#[test]
fn overridden_by_dir_returns_next_higher_priority_dir() {
    let d1 = TempDir::new("ci_overriddenby_d1");
    let d2 = TempDir::new("ci_overriddenby_d2");
    let d3 = TempDir::new("ci_overriddenby_d3");
    d1.write("shared.txt", b"");
    d2.write("shared.txt", b"");
    d3.write("shared.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path(), d3.path()]);
    let path = Path::new("shared.txt");

    // d1 (index 0) is directly overridden by d2 (index 1)
    assert_eq!(index.overridden_by_dir(0, path), Some(1));
    // d2 (index 1) is directly overridden by d3 (index 2)
    assert_eq!(index.overridden_by_dir(1, path), Some(2));
    // d3 (index 2) is overridden by nothing
    assert_eq!(index.overridden_by_dir(2, path), None);
}

#[test]
fn empty_directories_produce_no_conflicts() {
    let d1 = TempDir::new("ci_empty_d1");
    let d2 = TempDir::new("ci_empty_d2");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    assert!(!index.conflicts[0].has_overrides());
    assert!(!index.conflicts[0].is_overridden());
    assert!(!index.conflicts[1].has_overrides());
    assert!(!index.conflicts[1].is_overridden());
}

#[test]
fn single_directory_never_conflicts_with_itself() {
    let d1 = TempDir::new("ci_single_d1");
    d1.write("a.txt", b"");
    d1.write("b.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path()]);
    assert!(!index.conflicts[0].has_overrides());
    assert!(!index.conflicts[0].is_overridden());
}

#[test]
fn partial_overlap_correct_per_dir_counts() {
    let d1 = TempDir::new("ci_partial_d1");
    let d2 = TempDir::new("ci_partial_d2");
    d1.write("shared_a.txt", b"");
    d1.write("shared_b.txt", b"");
    d1.write("only_d1.txt", b"");
    d2.write("shared_a.txt", b"");
    d2.write("shared_b.txt", b"");
    d2.write("only_d2.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

    assert_eq!(index.conflicts[0].overridden_by.len(), 2);
    assert_eq!(index.conflicts[1].overrides.len(), 2);
    assert!(
        !index.conflicts[0]
            .overridden_by
            .contains(&PathBuf::from("only_d1.txt"))
    );
    assert!(
        !index.conflicts[1]
            .overrides
            .contains(&PathBuf::from("only_d2.txt"))
    );
}

#[test]
fn from_file_lists_produces_same_result_as_from_directories() {
    let d1 = TempDir::new("ci_ffl_d1");
    let d2 = TempDir::new("ci_ffl_d2");
    d1.write("shared.txt", b"");
    d1.write("only_d1.txt", b"");
    d2.write("shared.txt", b"");
    d2.write("only_d2.txt", b"");

    let from_dirs = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);

    // Build the same index manually via from_file_lists
    let lists = vec![
        (
            d1.path().to_path_buf(),
            vec![PathBuf::from("shared.txt"), PathBuf::from("only_d1.txt")],
        ),
        (
            d2.path().to_path_buf(),
            vec![PathBuf::from("shared.txt"), PathBuf::from("only_d2.txt")],
        ),
    ];
    let from_lists = ConflictIndex::from_file_lists(lists);

    assert_eq!(
        from_dirs.conflicts[0].overrides,
        from_lists.conflicts[0].overrides
    );
    assert_eq!(
        from_dirs.conflicts[0].overridden_by,
        from_lists.conflicts[0].overridden_by
    );
    assert_eq!(
        from_dirs.conflicts[1].overrides,
        from_lists.conflicts[1].overrides
    );
    assert_eq!(
        from_dirs.conflicts[1].overridden_by,
        from_lists.conflicts[1].overridden_by
    );
}

#[test]
fn from_file_lists_archive_before_dir_gives_archive_lower_priority() {
    // Simulate: archive provides "textures/foo.dds", dir overrides it.
    // Archive is index 0 (lower priority), dir is index 1 (higher priority).
    let lists = vec![
        (
            PathBuf::from("Morrowind.bsa"),
            vec![PathBuf::from("textures/foo.dds")],
        ),
        (
            PathBuf::from("/data/mod"),
            vec![PathBuf::from("textures/foo.dds")],
        ),
    ];
    let index = ConflictIndex::from_file_lists(lists);

    // archive (0): overridden by the dir, overrides nothing
    assert!(
        !index.conflicts[0].has_overrides(),
        "archive should not override anything"
    );
    assert!(
        index.conflicts[0].is_overridden(),
        "archive should be overridden by the dir"
    );

    // dir (1): overrides the archive, not overridden by anything
    assert!(
        index.conflicts[1].has_overrides(),
        "dir should override the archive"
    );
    assert!(
        !index.conflicts[1].is_overridden(),
        "nothing overrides the dir"
    );
}

// ---- conflicts_report ----

#[test]
fn conflicts_report_relative_paths() {
    let d1 = TempDir::new("ci_report_cr_d1");
    let d2 = TempDir::new("ci_report_cr_d2");
    d1.write("shared.txt", b"");
    d2.write("shared.txt", b"");
    d1.write("only1.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.conflicts_report(true);

    assert_eq!(report.sources.len(), 2);
    // d2 (idx 1) overrides shared.txt — its overrides list should contain the relative key
    let d2_entry = &report.sources[1];
    assert!(!d2_entry.overrides.is_empty(), "d2 should have overrides");
    assert!(
        d2_entry
            .overrides
            .iter()
            .any(|p| p == &PathBuf::from("shared.txt")),
        "overrides should contain the relative key 'shared.txt'"
    );
}

#[test]
fn conflicts_report_sorted() {
    let d1 = TempDir::new("ci_report_cr_sort_d1");
    let d2 = TempDir::new("ci_report_cr_sort_d2");
    d1.write("b.txt", b"");
    d1.write("a.txt", b"");
    d2.write("b.txt", b"");
    d2.write("a.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.conflicts_report(true);

    let overrides = &report.sources[1].overrides;
    assert!(
        overrides.windows(2).all(|w| w[0] <= w[1]),
        "overrides should be sorted"
    );
    let overridden_by = &report.sources[0].overridden_by;
    assert!(
        overridden_by.windows(2).all(|w| w[0] <= w[1]),
        "overridden_by should be sorted"
    );
}

#[test]
fn conflicts_report_formats_archive_sources_as_entries() {
    let layer = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/data/archive.zip"),
                kind: SourceKind::Archive,
            },
            vec![PathBuf::from("textures/foo.dds")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/data/mod"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("textures/foo.dds")],
        ),
    ]);
    let index = ConflictIndex::from_layer_index(&layer);

    let report = index.conflicts_report(false);
    assert_eq!(
        report.sources[0].overridden_by,
        vec![PathBuf::from("/data/archive.zip::textures/foo.dds")]
    );
}

// ---- shadowed_report ----

#[test]
fn shadowed_report_clean_source_excluded() {
    let d1 = TempDir::new("ci_report_sr_clean_d1");
    let d2 = TempDir::new("ci_report_sr_clean_d2");
    // No shared files — no conflicts
    d1.write("only1.txt", b"");
    d2.write("only2.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.shadowed_report(true);
    assert!(
        report.sources.is_empty(),
        "no sources should appear when there are no conflicts"
    );
}

#[test]
fn shadowed_report_overridden_source_included_sorted() {
    let d1 = TempDir::new("ci_report_sr_over_d1");
    let d2 = TempDir::new("ci_report_sr_over_d2");
    d1.write("b.txt", b"");
    d1.write("a.txt", b"");
    d2.write("b.txt", b"");
    d2.write("a.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.shadowed_report(true);

    // d1 is overridden by d2, so it should appear in the shadowed report
    assert!(!report.sources.is_empty(), "d1 should appear as shadowed");
    let shadowed = &report.sources[0];
    assert!(
        shadowed.shadowed_files.windows(2).all(|w| w[0] <= w[1]),
        "shadowed_files should be sorted"
    );
}

#[test]
fn shadowed_report_excludes_partially_overridden_source() {
    let d1 = TempDir::new("ci_report_sr_partial_d1");
    let d2 = TempDir::new("ci_report_sr_partial_d2");
    d1.write("shared.txt", b"");
    d1.write("unique.txt", b"");
    d2.write("shared.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.shadowed_report(true);

    assert!(report.sources.is_empty());
}

// ---- diff_report ----

#[test]
fn diff_report_shared_and_unique() {
    let d1 = TempDir::new("ci_report_dr_d1");
    let d2 = TempDir::new("ci_report_dr_d2");
    d1.write("shared.txt", b"");
    d1.write("only_a.txt", b"");
    d2.write("shared.txt", b"");
    d2.write("only_b.txt", b"");

    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.diff_report(d1.path(), d2.path());

    assert!(
        report.shared.contains(&PathBuf::from("shared.txt")),
        "shared.txt should be in shared"
    );
    assert!(
        report.only_in_a.contains(&PathBuf::from("only_a.txt")),
        "only_a.txt should be only in a"
    );
    assert!(
        report.only_in_b.contains(&PathBuf::from("only_b.txt")),
        "only_b.txt should be only in b"
    );
}

#[test]
fn diff_report_higher_priority_is_later_dir() {
    let d1 = TempDir::new("ci_report_dr_prio_d1");
    let d2 = TempDir::new("ci_report_dr_prio_d2");
    d1.write("f.txt", b"");
    d2.write("f.txt", b"");

    // d2 comes later, so it should have higher priority
    let index = ConflictIndex::from_directories(vec![d1.path(), d2.path()]);
    let report = index.diff_report(d1.path(), d2.path());

    assert_eq!(
        report.higher_priority,
        d2.path().to_path_buf(),
        "d2 (later) should have higher priority"
    );
}
