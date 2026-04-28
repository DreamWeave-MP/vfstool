// SPDX-License-Identifier: GPL-3.0-only
use super::{
    evaluate::{evaluate_constraints, has_unavoidable_unsat, move_count},
    types::{ConstraintViolation, SolveEvalContext},
};
use std::cmp::Ordering;

pub(super) fn stable_topological_sort(
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

pub(super) fn improve_candidate(
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

    if !violations.is_empty() {
        if let Some(exact) =
            best_satisfying_topological_order(current, eval.source_count, precedence_edges, eval)
        {
            *candidate = exact;
            violations.clear();
        }
    }

    violations
}

pub(super) struct LocalSearch<'a> {
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

pub(super) fn best_neighbor(
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

pub(super) fn compare_solution_quality(
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

pub(super) fn moved(order: &[usize], from: usize, to: usize) -> Vec<usize> {
    let mut out = order.to_vec();
    let item = out.remove(from);
    out.insert(to, item);
    out
}

pub(super) fn satisfies_precedence(order: &[usize], edges: &[(usize, usize)]) -> bool {
    let mut rank = vec![0usize; order.len()];
    for (pos, source_idx) in order.iter().copied().enumerate() {
        rank[source_idx] = pos;
    }
    edges.iter().all(|(a, b)| rank[*a] < rank[*b])
}

pub(super) fn best_satisfying_topological_order(
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

pub(super) struct ExactSearch<'a> {
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
