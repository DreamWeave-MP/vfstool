// SPDX-License-Identifier: GPL-3.0-only
mod compile;
mod evaluate;
mod search;
#[cfg(test)]
mod tests;
mod types;

use crate::analysis::LayerIndex;
use compile::{compile_constraints, precedence_edges, resolve_current_order, source_lookup};
use evaluate::{changed_winner_count, indices_to_paths, move_count, precedence_cycle_violations};
use search::{improve_candidate, stable_topological_sort};
use std::io;
use types::SolveEvalContext;

pub use types::{
    ConstraintViolation, OrderConstraint, SolveDiagnostics, SolveObjective, SolveRequest,
    SolveResult, SolveStatus,
};

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
