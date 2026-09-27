// SPDX-License-Identifier: MIT OR Apache-2.0
use super::types::{CompiledConstraint, ConstraintViolation, OrderConstraint, SolveEvalContext};
use crate::{SourceKind, analysis::LayerIndex};
use std::path::PathBuf;

pub(super) fn evaluate_constraints(
    order: &[usize],
    eval: &SolveEvalContext<'_>,
) -> Vec<ConstraintViolation> {
    let mut violations = Vec::new();
    let mut rank = vec![0usize; eval.source_count];
    for (pos, source_idx) in order.iter().copied().enumerate() {
        rank[source_idx] = pos;
    }

    for constraint in eval.constraints {
        match constraint {
            CompiledConstraint::SourceBefore {
                constraint_index,
                a_idx,
                b_idx,
                a,
                b,
            } => {
                if rank[*a_idx] >= rank[*b_idx] {
                    violations.push(ConstraintViolation {
                        constraint_index: *constraint_index,
                        message: format!(
                            "source '{}' is not before '{}'",
                            a.display(),
                            b.display()
                        ),
                        sample_key: None,
                    });
                }
            }
            CompiledConstraint::SourceAfter {
                constraint_index,
                a_idx,
                b_idx,
                a,
                b,
            } => {
                if rank[*a_idx] <= rank[*b_idx] {
                    violations.push(ConstraintViolation {
                        constraint_index: *constraint_index,
                        message: format!("source '{}' is not after '{}'", a.display(), b.display()),
                        sample_key: None,
                    });
                }
            }
            CompiledConstraint::WinnerMustBe {
                constraint_index,
                path_glob,
                source_glob,
                matched_key_indices,
                allowed_sources,
            } => {
                let matched_keys = matched_key_indices.len();
                let mut failing_keys = Vec::new();
                for key_index in matched_key_indices {
                    let providers = eval.providers_by_key[*key_index];
                    let winner_idx = winner_for_providers(providers, &rank, eval.source_kinds);
                    let Some(winner_idx) = winner_idx else {
                        continue;
                    };

                    if !allowed_sources[winner_idx] {
                        failing_keys.push(eval.keys[*key_index].clone());
                    }
                }

                if matched_keys == 0 {
                    violations.push(ConstraintViolation {
                        constraint_index: *constraint_index,
                        message: format!("winner_must_be matched no keys for glob '{path_glob}'"),
                        sample_key: None,
                    });
                } else {
                    violations.extend(failing_keys.into_iter().map(|sample_key| {
                        ConstraintViolation {
                            constraint_index: *constraint_index,
                            message: format!(
                                "winner for matching key does not satisfy source glob '{source_glob}'"
                            ),
                            sample_key: Some(sample_key),
                        }
                    }));
                }
            }
        }
    }

    violations
}

pub(super) fn precedence_cycle_violations(
    constraints: &[OrderConstraint],
) -> Vec<ConstraintViolation> {
    constraints
        .iter()
        .enumerate()
        .filter_map(|(idx, constraint)| match constraint {
            OrderConstraint::SourceBefore { .. } | OrderConstraint::SourceAfter { .. } => {
                Some(ConstraintViolation {
                    constraint_index: idx,
                    message: "precedence constraints contain a cycle".into(),
                    sample_key: None,
                })
            }
            OrderConstraint::WinnerMustBe { .. } => None,
        })
        .collect()
}

pub(super) fn has_unavoidable_unsat(
    source_count: usize,
    constraints: &[CompiledConstraint],
    providers_by_key: &[&[usize]],
) -> bool {
    let winner_constraints: Vec<(&Vec<usize>, &Vec<bool>)> = constraints
        .iter()
        .filter_map(|constraint| match constraint {
            CompiledConstraint::WinnerMustBe {
                matched_key_indices,
                allowed_sources,
                ..
            } => Some((matched_key_indices, allowed_sources)),
            CompiledConstraint::SourceBefore { .. } | CompiledConstraint::SourceAfter { .. } => {
                None
            }
        })
        .collect();

    for (matched_keys, allowed_sources) in &winner_constraints {
        if matched_keys.is_empty() {
            return true;
        }
        for key_index in *matched_keys {
            let providers = providers_by_key[*key_index];
            if providers
                .iter()
                .all(|source_idx| !allowed_sources[*source_idx])
            {
                return true;
            }
        }
    }

    let mut combined_allowed = vec![true; source_count];
    for (key_index, providers) in providers_by_key.iter().enumerate() {
        let mut relevant = false;
        combined_allowed.fill(true);

        for (matched_keys, allowed_sources) in &winner_constraints {
            if matched_keys.binary_search(&key_index).is_ok() {
                relevant = true;
                for source_idx in 0..source_count {
                    combined_allowed[source_idx] &= allowed_sources[source_idx];
                }
            }
        }

        if relevant
            && providers
                .iter()
                .all(|source_idx| !combined_allowed[*source_idx])
        {
            return true;
        }
    }

    false
}

pub(super) fn move_count(current: &[usize], solved: &[usize]) -> usize {
    current
        .iter()
        .zip(solved.iter())
        .filter(|(a, b)| a != b)
        .count()
}

pub(super) fn changed_winner_count(
    source_count: usize,
    providers_by_key: &[&[usize]],
    source_kinds: &[SourceKind],
    current: &[usize],
    solved: &[usize],
) -> usize {
    let mut current_rank = vec![0usize; source_count];
    for (pos, source) in current.iter().copied().enumerate() {
        current_rank[source] = pos;
    }
    let mut solved_rank = vec![0usize; source_count];
    for (pos, source) in solved.iter().copied().enumerate() {
        solved_rank[source] = pos;
    }

    let mut changed = 0usize;
    for providers in providers_by_key {
        let current_winner = winner_for_providers(providers, &current_rank, source_kinds);
        let solved_winner = winner_for_providers(providers, &solved_rank, source_kinds);
        if current_winner != solved_winner {
            changed += 1;
        }
    }
    changed
}

pub(super) fn winner_for_providers(
    providers: &[usize],
    rank_by_source: &[usize],
    source_kinds: &[SourceKind],
) -> Option<usize> {
    providers
        .iter()
        .copied()
        .filter(|idx| source_kinds[*idx] == SourceKind::LooseDir)
        .max_by_key(|idx| rank_by_source[*idx])
        .or_else(|| {
            providers
                .iter()
                .copied()
                .max_by_key(|idx| rank_by_source[*idx])
        })
}

pub(super) fn indices_to_paths(layer: &LayerIndex, order: &[usize]) -> Vec<PathBuf> {
    order
        .iter()
        .map(|idx| layer.sources[*idx].path.clone())
        .collect()
}
