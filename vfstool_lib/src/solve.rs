// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{analysis::LayerIndex, path_glob_matches, source_glob_matches};
use std::{cmp::Ordering, io, path::PathBuf};

/// Optimization objective for solver output ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum SolveObjective {
    /// Prefer minimal reordering distance from current order.
    MinMovesFromCurrent,
}

/// Constraint declaration for load-order solving.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum OrderConstraint {
    /// Require source `a` to come before source `b`.
    SourceBefore {
        /// Source path that must come first.
        a: PathBuf,
        /// Source path that must come later.
        b: PathBuf,
    },
    /// Require source `a` to come after source `b`.
    SourceAfter {
        /// Source path that must come later.
        a: PathBuf,
        /// Source path that must come first.
        b: PathBuf,
    },
    /// Require matching keys to be won by a matching source.
    WinnerMustBe {
        /// Glob over normalized VFS keys.
        path_glob: String,
        /// Glob over source paths.
        source_glob: String,
    },
}

/// Solver request.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SolveRequest {
    /// Optional current order. If empty, layer source order is used.
    pub current_order: Vec<PathBuf>,
    /// Constraints to satisfy.
    pub constraints: Vec<OrderConstraint>,
    /// Optimization objective.
    pub objective: SolveObjective,
}

/// Solver status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum SolveStatus {
    /// A valid order was found.
    Satisfiable,
    /// No valid order exists under provided constraints.
    Unsatisfiable,
}

/// One violated constraint description.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ConstraintViolation {
    /// Constraint index in the request list.
    pub constraint_index: usize,
    /// Human-readable reason.
    pub message: String,
    /// Optional sample key that demonstrates failure.
    pub sample_key: Option<PathBuf>,
}

/// Solver diagnostics.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SolveDiagnostics {
    /// Violations for unsatisfied constraints.
    pub violated_constraints: Vec<ConstraintViolation>,
    /// Number of moved positions from current order.
    pub move_count: usize,
    /// Number of keys whose winner changed from current order.
    pub changed_winners: usize,
}

/// Solver output.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SolveResult {
    /// Whether the constraint set is satisfiable.
    pub status: SolveStatus,
    /// Suggested order when satisfiable.
    pub order: Option<Vec<PathBuf>>,
    /// Additional diagnostics.
    pub diagnostics: SolveDiagnostics,
}

impl LayerIndex {
    /// Solve constraints and suggest an improved load order.
    ///
    /// # Errors
    ///
    /// Returns an error if request references unknown or invalid sources.
    pub fn solve_order(&self, request: &SolveRequest) -> io::Result<SolveResult> {
        match request.objective {
            SolveObjective::MinMovesFromCurrent => {}
        }

        let current = resolve_current_order(self, &request.current_order)?;
        let precedence_edges = build_precedence_edges(self, &request.constraints)?;

        let Some(mut candidate) =
            stable_topological_sort(&current, self.sources.len(), &precedence_edges)
        else {
            return Ok(SolveResult {
                status: SolveStatus::Unsatisfiable,
                order: None,
                diagnostics: SolveDiagnostics {
                    violated_constraints: precedence_cycle_violations(&request.constraints),
                    move_count: 0,
                    changed_winners: 0,
                },
            });
        };

        let mut violations = evaluate_constraints(self, &candidate, &request.constraints);
        if !violations.is_empty() {
            let max_iters = self.sources.len().saturating_mul(self.sources.len()).max(1);
            for _ in 0..max_iters {
                let Some(next) = best_neighbor(
                    self,
                    &current,
                    &candidate,
                    &precedence_edges,
                    &request.constraints,
                ) else {
                    break;
                };

                let next_violations = evaluate_constraints(self, &next, &request.constraints);
                if compare_solution_quality(
                    &next_violations,
                    &next,
                    &violations,
                    &candidate,
                    &current,
                ) == Ordering::Less
                {
                    candidate = next;
                    violations = next_violations;
                } else {
                    break;
                }
            }
        }

        let move_count = move_count(&current, &candidate);
        let changed_winners = changed_winner_count(self, &current, &candidate);

        let status = if violations.is_empty() {
            SolveStatus::Satisfiable
        } else {
            SolveStatus::Unsatisfiable
        };

        let order = if status == SolveStatus::Satisfiable {
            Some(indices_to_paths(self, &candidate))
        } else {
            None
        };

        Ok(SolveResult {
            status,
            order,
            diagnostics: SolveDiagnostics {
                violated_constraints: violations,
                move_count,
                changed_winners,
            },
        })
    }
}

fn resolve_current_order(layer: &LayerIndex, current_order: &[PathBuf]) -> io::Result<Vec<usize>> {
    if current_order.is_empty() {
        return Ok((0..layer.sources.len()).collect());
    }

    if current_order.len() != layer.sources.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "current_order length does not match source count",
        ));
    }

    let mut seen = vec![false; layer.sources.len()];
    let mut indices = Vec::with_capacity(current_order.len());
    for path in current_order {
        let idx = layer
            .sources
            .iter()
            .position(|source| source.path == *path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown source in current_order: {}", path.display()),
                )
            })?;

        if seen[idx] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate source in current_order: {}", path.display()),
            ));
        }
        seen[idx] = true;
        indices.push(idx);
    }

    Ok(indices)
}

fn build_precedence_edges(
    layer: &LayerIndex,
    constraints: &[OrderConstraint],
) -> io::Result<Vec<(usize, usize)>> {
    let mut edges = Vec::new();
    for constraint in constraints {
        let maybe_edge = match constraint {
            OrderConstraint::SourceBefore { a, b } => {
                Some((source_index(layer, a)?, source_index(layer, b)?))
            }
            OrderConstraint::SourceAfter { a, b } => {
                Some((source_index(layer, b)?, source_index(layer, a)?))
            }
            OrderConstraint::WinnerMustBe { .. } => None,
        };
        if let Some(edge) = maybe_edge {
            edges.push(edge);
        }
    }
    Ok(edges)
}

fn source_index(layer: &LayerIndex, path: &PathBuf) -> io::Result<usize> {
    layer
        .sources
        .iter()
        .position(|source| source.path == *path)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown source in constraint: {}", path.display()),
            )
        })
}

fn stable_topological_sort(
    current: &[usize],
    node_count: usize,
    edges: &[(usize, usize)],
) -> Option<Vec<usize>> {
    let mut indegree = vec![0usize; node_count];
    let mut outgoing = vec![Vec::<usize>::new(); node_count];
    for &(from, to) in edges {
        outgoing[from].push(to);
        indegree[to] += 1;
    }

    let mut current_rank = vec![0usize; node_count];
    for (rank, source) in current.iter().copied().enumerate() {
        current_rank[source] = rank;
    }

    let mut used = vec![false; node_count];
    let mut order = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let mut candidate = None;
        for node in 0..node_count {
            if !used[node] && indegree[node] == 0 {
                candidate = match candidate {
                    None => Some(node),
                    Some(best) => {
                        if current_rank[node] < current_rank[best] {
                            Some(node)
                        } else {
                            Some(best)
                        }
                    }
                };
            }
        }

        let node = candidate?;
        used[node] = true;
        order.push(node);
        for &next in &outgoing[node] {
            indegree[next] = indegree[next].saturating_sub(1);
        }
    }

    Some(order)
}

fn evaluate_constraints(
    layer: &LayerIndex,
    order: &[usize],
    constraints: &[OrderConstraint],
) -> Vec<ConstraintViolation> {
    let mut violations = Vec::new();
    let mut rank = vec![0usize; layer.sources.len()];
    for (pos, source_idx) in order.iter().copied().enumerate() {
        rank[source_idx] = pos;
    }

    for (idx, constraint) in constraints.iter().enumerate() {
        match constraint {
            OrderConstraint::SourceBefore { a, b } => {
                let Some(ai) = layer.sources.iter().position(|s| s.path == *a) else {
                    continue;
                };
                let Some(bi) = layer.sources.iter().position(|s| s.path == *b) else {
                    continue;
                };
                if rank[ai] >= rank[bi] {
                    violations.push(ConstraintViolation {
                        constraint_index: idx,
                        message: format!(
                            "source '{}' is not before '{}'",
                            a.display(),
                            b.display()
                        ),
                        sample_key: None,
                    });
                }
            }
            OrderConstraint::SourceAfter { a, b } => {
                let Some(ai) = layer.sources.iter().position(|s| s.path == *a) else {
                    continue;
                };
                let Some(bi) = layer.sources.iter().position(|s| s.path == *b) else {
                    continue;
                };
                if rank[ai] <= rank[bi] {
                    violations.push(ConstraintViolation {
                        constraint_index: idx,
                        message: format!("source '{}' is not after '{}'", a.display(), b.display()),
                        sample_key: None,
                    });
                }
            }
            OrderConstraint::WinnerMustBe {
                path_glob,
                source_glob,
            } => {
                let mut matched_keys = 0usize;
                let mut failing_key = None;
                for key in layer.keys() {
                    if !path_glob_matches(path_glob, &key) {
                        continue;
                    }
                    matched_keys += 1;

                    let providers = layer.sources_containing(&key);
                    let winner_idx = providers.iter().copied().max_by_key(|src| rank[*src]);
                    let Some(winner_idx) = winner_idx else {
                        continue;
                    };

                    let winner = &layer.sources[winner_idx];
                    if !source_glob_matches(source_glob, &winner.path) {
                        failing_key = Some(key);
                        break;
                    }
                }

                if matched_keys == 0 {
                    violations.push(ConstraintViolation {
                        constraint_index: idx,
                        message: format!("winner_must_be matched no keys for glob '{path_glob}'"),
                        sample_key: None,
                    });
                } else if let Some(sample_key) = failing_key {
                    violations.push(ConstraintViolation {
                        constraint_index: idx,
                        message: format!(
                            "winner for matching keys does not satisfy source glob '{source_glob}'"
                        ),
                        sample_key: Some(sample_key),
                    });
                }
            }
        }
    }

    violations
}

fn precedence_cycle_violations(constraints: &[OrderConstraint]) -> Vec<ConstraintViolation> {
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

fn best_neighbor(
    layer: &LayerIndex,
    current: &[usize],
    order: &[usize],
    precedence_edges: &[(usize, usize)],
    constraints: &[OrderConstraint],
) -> Option<Vec<usize>> {
    let baseline_violations = evaluate_constraints(layer, order, constraints);
    let mut best = None;
    let mut best_violations = baseline_violations;

    for i in 0..order.len() {
        for j in 0..order.len() {
            if i == j {
                continue;
            }
            let candidate = moved(order, i, j);
            if !satisfies_precedence(&candidate, precedence_edges) {
                continue;
            }
            let candidate_violations = evaluate_constraints(layer, &candidate, constraints);
            if compare_solution_quality(
                &candidate_violations,
                &candidate,
                &best_violations,
                order,
                current,
            ) == Ordering::Less
            {
                best = Some(candidate);
                best_violations = candidate_violations;
            }
        }
    }

    best
}

fn compare_solution_quality(
    lhs_violations: &[ConstraintViolation],
    lhs_order: &[usize],
    rhs_violations: &[ConstraintViolation],
    rhs_order: &[usize],
    current: &[usize],
) -> Ordering {
    lhs_violations
        .len()
        .cmp(&rhs_violations.len())
        .then_with(|| move_count(current, lhs_order).cmp(&move_count(current, rhs_order)))
}

fn moved(order: &[usize], from: usize, to: usize) -> Vec<usize> {
    let mut out = order.to_vec();
    let item = out.remove(from);
    out.insert(to, item);
    out
}

fn satisfies_precedence(order: &[usize], edges: &[(usize, usize)]) -> bool {
    let mut rank = vec![0usize; order.len()];
    for (pos, source_idx) in order.iter().copied().enumerate() {
        rank[source_idx] = pos;
    }
    edges.iter().all(|(a, b)| rank[*a] < rank[*b])
}

fn move_count(current: &[usize], solved: &[usize]) -> usize {
    current
        .iter()
        .zip(solved.iter())
        .filter(|(a, b)| a != b)
        .count()
}

fn changed_winner_count(layer: &LayerIndex, current: &[usize], solved: &[usize]) -> usize {
    let mut current_rank = vec![0usize; layer.sources.len()];
    for (pos, source) in current.iter().copied().enumerate() {
        current_rank[source] = pos;
    }
    let mut solved_rank = vec![0usize; layer.sources.len()];
    for (pos, source) in solved.iter().copied().enumerate() {
        solved_rank[source] = pos;
    }

    let mut changed = 0usize;
    for key in layer.keys() {
        let providers = layer.sources_containing(&key);
        let current_winner = providers
            .iter()
            .copied()
            .max_by_key(|src| current_rank[*src]);
        let solved_winner = providers
            .iter()
            .copied()
            .max_by_key(|src| solved_rank[*src]);
        if current_winner != solved_winner {
            changed += 1;
        }
    }
    changed
}

fn indices_to_paths(layer: &LayerIndex, order: &[usize]) -> Vec<PathBuf> {
    order
        .iter()
        .map(|idx| layer.sources[*idx].path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceKind, analysis::SourceMeta};

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
}
