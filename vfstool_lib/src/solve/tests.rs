// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
use crate::{SourceKind, analysis::SourceMeta};
use std::{io, path::PathBuf};

fn sample_layer() -> LayerIndex {
    LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/a"),
                kind: SourceKind::LooseDir,
            },
            vec![
                PathBuf::from("scripts/x.lua"),
                PathBuf::from("textures/a.dds"),
            ],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/b"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("scripts/x.lua")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/c"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("textures/a.dds")],
        ),
    ])
}

#[test]
fn solve_precedence_constraints() {
    let layer = sample_layer();
    let result = layer
        .solve_order(&SolveRequest {
            current_order: vec![
                PathBuf::from("/b"),
                PathBuf::from("/a"),
                PathBuf::from("/c"),
            ],
            constraints: vec![OrderConstraint::SourceBefore {
                a: PathBuf::from("/a"),
                b: PathBuf::from("/c"),
            }],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect("solve should succeed");

    assert_eq!(result.status, SolveStatus::Satisfiable);
    assert!(result.order.is_some());
}

#[test]
fn solve_detects_precedence_cycle() {
    let layer = sample_layer();
    let result = layer
        .solve_order(&SolveRequest {
            current_order: vec![],
            constraints: vec![
                OrderConstraint::SourceBefore {
                    a: PathBuf::from("/a"),
                    b: PathBuf::from("/b"),
                },
                OrderConstraint::SourceBefore {
                    a: PathBuf::from("/b"),
                    b: PathBuf::from("/a"),
                },
            ],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect("solve should return result");

    assert_eq!(result.status, SolveStatus::Unsatisfiable);
    assert!(result.order.is_none());
    assert!(!result.diagnostics.violated_constraints.is_empty());
}

#[test]
fn solve_winner_constraint_by_reordering() {
    let layer = sample_layer();
    let result = layer
        .solve_order(&SolveRequest {
            current_order: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
            constraints: vec![OrderConstraint::WinnerMustBe {
                path_glob: "scripts/**".into(),
                source_glob: "**/a".into(),
            }],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect("solve should succeed");

    assert_eq!(result.status, SolveStatus::Satisfiable);
    let solved = result.order.expect("expected solved order");
    let pos_a = solved
        .iter()
        .position(|p| p == &PathBuf::from("/a"))
        .expect("a should exist");
    let pos_b = solved
        .iter()
        .position(|p| p == &PathBuf::from("/b"))
        .expect("b should exist");
    assert!(pos_a > pos_b);
}

#[test]
fn solve_winner_constraint_uses_exact_fallback_for_neutral_moves() {
    let layer = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/allowed_a"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("one.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/blocked_b"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("one.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/allowed_d"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("two.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/blocked_c"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("two.txt")],
        ),
    ]);

    let result = layer
        .solve_order(&SolveRequest {
            current_order: vec![],
            constraints: vec![OrderConstraint::WinnerMustBe {
                path_glob: "**/*.txt".into(),
                source_glob: "**/allowed_*".into(),
            }],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect("solve should succeed");

    assert_eq!(result.status, SolveStatus::Satisfiable);
}

#[test]
fn solve_winner_constraint_cannot_make_archive_beat_loose_file() {
    let layer = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/archive.bsa"),
                kind: SourceKind::Archive,
            },
            vec![PathBuf::from("textures/a.dds")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/loose"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("textures/a.dds")],
        ),
    ]);

    let result = layer
        .solve_order(&SolveRequest {
            current_order: vec![],
            constraints: vec![OrderConstraint::WinnerMustBe {
                path_glob: "textures/a.dds".into(),
                source_glob: "**/archive.bsa".into(),
            }],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect("solve should return result");

    assert_eq!(result.status, SolveStatus::Unsatisfiable);
}

#[test]
fn solve_unsat_contradictory_winner_constraints() {
    let layer = sample_layer();
    let result = layer
        .solve_order(&SolveRequest {
            current_order: vec![],
            constraints: vec![
                OrderConstraint::WinnerMustBe {
                    path_glob: "scripts/x.lua".into(),
                    source_glob: "**/a".into(),
                },
                OrderConstraint::WinnerMustBe {
                    path_glob: "scripts/x.lua".into(),
                    source_glob: "**/b".into(),
                },
            ],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect("solve should return result");

    assert_eq!(result.status, SolveStatus::Unsatisfiable);
    assert!(result.order.is_none());
    assert!(!result.diagnostics.violated_constraints.is_empty());
}

#[test]
fn solve_unknown_source_in_constraint_errors() {
    let layer = sample_layer();
    let err = layer
        .solve_order(&SolveRequest {
            current_order: vec![],
            constraints: vec![OrderConstraint::SourceBefore {
                a: PathBuf::from("/does-not-exist"),
                b: PathBuf::from("/a"),
            }],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect_err("unknown source constraint should error");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("unknown source in constraint"));
}

#[test]
fn solve_unknown_source_in_current_order_errors() {
    let layer = sample_layer();
    let err = layer
        .solve_order(&SolveRequest {
            current_order: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/missing"),
            ],
            constraints: vec![],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect_err("unknown source in current order should error");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("unknown source in current_order"));
}

#[test]
fn solve_rejects_duplicate_source_paths() {
    let layer = LayerIndex::from_file_lists(vec![
        (
            SourceMeta {
                path: PathBuf::from("/dup"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("a.txt")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("/dup"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("b.txt")],
        ),
    ]);

    let err = layer
        .solve_order(&SolveRequest {
            current_order: vec![],
            constraints: vec![],
            objective: SolveObjective::MinMovesFromCurrent,
        })
        .expect_err("duplicate source paths should be rejected");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("duplicate source path"));
}

#[test]
fn solve_is_deterministic_across_runs() {
    let layer = sample_layer();
    let request = SolveRequest {
        current_order: vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ],
        constraints: vec![
            OrderConstraint::WinnerMustBe {
                path_glob: "scripts/**".into(),
                source_glob: "**/a".into(),
            },
            OrderConstraint::SourceBefore {
                a: PathBuf::from("/b"),
                b: PathBuf::from("/a"),
            },
        ],
        objective: SolveObjective::MinMovesFromCurrent,
    };

    let first = layer
        .solve_order(&request)
        .expect("first solve should succeed");
    let second = layer
        .solve_order(&request)
        .expect("second solve should succeed");

    assert_eq!(first.status, second.status);
    assert_eq!(first.order, second.order);
    assert_eq!(
        first.diagnostics.violated_constraints.len(),
        second.diagnostics.violated_constraints.len()
    );
    assert_eq!(first.diagnostics.move_count, second.diagnostics.move_count);
    assert_eq!(
        first.diagnostics.changed_winners,
        second.diagnostics.changed_winners
    );
}
