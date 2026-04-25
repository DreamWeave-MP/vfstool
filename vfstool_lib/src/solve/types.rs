// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::SourceKind;
use std::path::PathBuf;

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
pub(super) enum CompiledConstraint {
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

pub(super) struct SolveEvalContext<'a> {
    pub(super) source_count: usize,
    pub(super) constraints: &'a [CompiledConstraint],
    pub(super) keys: &'a [PathBuf],
    pub(super) providers_by_key: &'a [&'a [usize]],
    pub(super) source_kinds: &'a [SourceKind],
}
