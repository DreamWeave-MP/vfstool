// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{SourceKind, analysis::LayerIndex, matchers::CompiledGlob};
use ahash::AHashMap;
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

#[derive(Debug, Clone)]
enum CompiledConstraint {
    SourceBefore {
        constraint_index: usize,
        a_idx: usize,
        b_idx: usize,
        a: PathBuf,
        b: PathBuf,
    },
    SourceAfter {
        constraint_index: usize,
        a_idx: usize,
        b_idx: usize,
        a: PathBuf,
        b: PathBuf,
    },
    WinnerMustBe {
        constraint_index: usize,
        path_glob: String,
        source_glob: String,
        matched_key_indices: Vec<usize>,
        allowed_sources: Vec<bool>,
    },
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

        let source_lookup = source_lookup(self)?;
        let current = resolve_current_order(self, &request.current_order, &source_lookup)?;
        let keys = self.keys();
        let providers_by_key: Vec<&[usize]> = keys
            .iter()
            .map(|key| self.sources_containing(key))
            .collect();
        let source_kinds = self
            .sources
            .iter()
            .map(|source| source.kind)
            .collect::<Vec<_>>();
        let compiled_constraints =
            compile_constraints(self, &request.constraints, &keys, &source_lookup)?;
        let precedence_edges = precedence_edges(&compiled_constraints);

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

        let eval = SolveEvalContext {
            source_count: self.sources.len(),
            constraints: &compiled_constraints,
            keys: &keys,
            providers_by_key: &providers_by_key,
            source_kinds: &source_kinds,
        };

        let violations = improve_candidate(&current, &mut candidate, &precedence_edges, &eval);

        let move_count = move_count(&current, &candidate);
        let changed_winners = changed_winner_count(
            self.sources.len(),
            &providers_by_key,
            &source_kinds,
            &current,
            &candidate,
        );

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

fn source_lookup(layer: &LayerIndex) -> io::Result<AHashMap<PathBuf, usize>> {
    let mut lookup = AHashMap::new();
    for (idx, source) in layer.sources.iter().enumerate() {
        if lookup.insert(source.path.clone(), idx).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate source path: {}", source.path.display()),
            ));
        }
    }
    Ok(lookup)
}

fn resolve_current_order(
    layer: &LayerIndex,
    current_order: &[PathBuf],
    source_lookup: &AHashMap<PathBuf, usize>,
) -> io::Result<Vec<usize>> {
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
        let idx = source_lookup.get(path).copied().ok_or_else(|| {
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

fn precedence_edges(constraints: &[CompiledConstraint]) -> Vec<(usize, usize)> {
    constraints
        .iter()
        .filter_map(|constraint| match constraint {
            CompiledConstraint::SourceBefore { a_idx, b_idx, .. } => Some((*a_idx, *b_idx)),
            CompiledConstraint::SourceAfter { a_idx, b_idx, .. } => Some((*b_idx, *a_idx)),
            CompiledConstraint::WinnerMustBe { .. } => None,
        })
        .collect()
}

fn compile_constraints(
    layer: &LayerIndex,
    constraints: &[OrderConstraint],
    keys: &[PathBuf],
    source_lookup: &AHashMap<PathBuf, usize>,
) -> io::Result<Vec<CompiledConstraint>> {
    let mut compiled = Vec::with_capacity(constraints.len());
    for (constraint_index, constraint) in constraints.iter().enumerate() {
        let item = match constraint {
            OrderConstraint::SourceBefore { a, b } => CompiledConstraint::SourceBefore {
                constraint_index,
                a_idx: source_index(source_lookup, a)?,
                b_idx: source_index(source_lookup, b)?,
                a: a.clone(),
                b: b.clone(),
            },
            OrderConstraint::SourceAfter { a, b } => CompiledConstraint::SourceAfter {
                constraint_index,
                a_idx: source_index(source_lookup, a)?,
                b_idx: source_index(source_lookup, b)?,
                a: a.clone(),
                b: b.clone(),
            },
            OrderConstraint::WinnerMustBe {
                path_glob,
                source_glob,
            } => {
                let path_glob_re = compile_glob("path_glob", path_glob)?;
                let matched_key_indices = keys
                    .iter()
                    .enumerate()
                    .filter_map(|(key_idx, key)| {
                        if path_glob_re.is_match(key) {
                            Some(key_idx)
                        } else {
                            None
                        }
                    })
                    .collect();
                let source_glob_re = compile_glob("source_glob", source_glob)?;
                let allowed_sources = layer
                    .sources
                    .iter()
                    .map(|source| source_glob_re.is_match(&source.path))
                    .collect();

                CompiledConstraint::WinnerMustBe {
                    constraint_index,
                    path_glob: path_glob.clone(),
                    source_glob: source_glob.clone(),
                    matched_key_indices,
                    allowed_sources,
                }
            }
        };
        compiled.push(item);
    }
    Ok(compiled)
}

fn compile_glob(field: &str, glob: &str) -> io::Result<CompiledGlob> {
    CompiledGlob::new(glob).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {field} '{glob}': {err}"),
        )
    })
}

fn source_index(source_lookup: &AHashMap<PathBuf, usize>, path: &PathBuf) -> io::Result<usize> {
    source_lookup.get(path).copied().ok_or_else(|| {
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

fn evaluate_constraints(order: &[usize], eval: &SolveEvalContext<'_>) -> Vec<ConstraintViolation> {
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

fn has_unavoidable_unsat(
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

struct SolveEvalContext<'a> {
    source_count: usize,
    constraints: &'a [CompiledConstraint],
    keys: &'a [PathBuf],
    providers_by_key: &'a [&'a [usize]],
    source_kinds: &'a [SourceKind],
}

fn improve_candidate(
    current: &[usize],
    candidate: &mut Vec<usize>,
    precedence_edges: &[(usize, usize)],
    eval: &SolveEvalContext<'_>,
) -> Vec<ConstraintViolation> {
    let mut violations = evaluate_constraints(candidate, eval);
    if violations.is_empty()
        || has_unavoidable_unsat(eval.source_count, eval.constraints, eval.providers_by_key)
    {
        return violations;
    }

    let search = LocalSearch {
        current,
        precedence_edges,
        eval,
    };
    search.improve_candidate_locally(candidate, &mut violations);

    if !violations.is_empty()
        && let Some(exact) =
            best_satisfying_topological_order(current, eval.source_count, precedence_edges, eval)
    {
        *candidate = exact;
        violations.clear();
    }

    violations
}

struct LocalSearch<'a> {
    current: &'a [usize],
    precedence_edges: &'a [(usize, usize)],
    eval: &'a SolveEvalContext<'a>,
}

impl LocalSearch<'_> {
    fn improve_candidate_locally(
        &self,
        candidate: &mut Vec<usize>,
        violations: &mut Vec<ConstraintViolation>,
    ) {
        let max_iters = self
            .eval
            .source_count
            .saturating_mul(self.eval.source_count)
            .max(1);
        for _ in 0..max_iters {
            let Some(next) =
                best_neighbor(self.current, candidate, self.precedence_edges, self.eval)
            else {
                break;
            };

            let next_violations = evaluate_constraints(&next, self.eval);
            if compare_solution_quality(
                &next_violations,
                &next,
                violations,
                candidate,
                self.current,
            ) != Ordering::Less
            {
                break;
            }

            *candidate = next;
            *violations = next_violations;
        }
    }
}

fn best_neighbor(
    current: &[usize],
    order: &[usize],
    precedence_edges: &[(usize, usize)],
    eval: &SolveEvalContext<'_>,
) -> Option<Vec<usize>> {
    let baseline_violations = evaluate_constraints(order, eval);
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
            let candidate_violations = evaluate_constraints(&candidate, eval);
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

fn best_satisfying_topological_order(
    current: &[usize],
    source_count: usize,
    precedence_edges: &[(usize, usize)],
    eval: &SolveEvalContext<'_>,
) -> Option<Vec<usize>> {
    const MAX_EXACT_SOURCES: usize = 9;
    if source_count > MAX_EXACT_SOURCES {
        return None;
    }

    let mut indegree = vec![0usize; source_count];
    let mut outgoing = vec![Vec::<usize>::new(); source_count];
    for &(from, to) in precedence_edges {
        outgoing[from].push(to);
        indegree[to] += 1;
    }

    let mut used = vec![false; source_count];
    let mut order = Vec::with_capacity(source_count);
    let mut best: Option<Vec<usize>> = None;
    let search = ExactSearch {
        current,
        source_count,
        eval,
        outgoing: &outgoing,
    };
    search.search(&mut indegree, &mut used, &mut order, &mut best);
    best
}

struct ExactSearch<'a> {
    current: &'a [usize],
    source_count: usize,
    eval: &'a SolveEvalContext<'a>,
    outgoing: &'a [Vec<usize>],
}

impl ExactSearch<'_> {
    fn search(
        &self,
        indegree: &mut [usize],
        used: &mut [bool],
        order: &mut Vec<usize>,
        best: &mut Option<Vec<usize>>,
    ) {
        if order.len() == self.source_count {
            if evaluate_constraints(order, self.eval).is_empty()
                && best.as_ref().is_none_or(|best_order| {
                    move_count(self.current, order) < move_count(self.current, best_order)
                })
            {
                *best = Some(order.clone());
            }
            return;
        }

        for node in 0..self.source_count {
            if used[node] || indegree[node] != 0 {
                continue;
            }

            used[node] = true;
            order.push(node);
            for &next in &self.outgoing[node] {
                indegree[next] = indegree[next].saturating_sub(1);
            }

            self.search(indegree, used, order, best);

            for &next in &self.outgoing[node] {
                indegree[next] += 1;
            }
            order.pop();
            used[node] = false;
        }
    }
}

fn move_count(current: &[usize], solved: &[usize]) -> usize {
    current
        .iter()
        .zip(solved.iter())
        .filter(|(a, b)| a != b)
        .count()
}

fn changed_winner_count(
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

fn winner_for_providers(
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
}
