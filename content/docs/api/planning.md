+++
title = "Simulation and planning"
description = "What-if questions about a load order: which winners a reorder would change and how much that matters, what adding a directory would displace, and a solver for order constraints."
weight = 110

[extra]
kind = "api"
+++

Three questions a `LayerIndex` can answer before anything is moved: what would change if the load
order changed (`simulate`, `simulate_impact`), what a new directory on top would displace
(`plan_candidate_directory`), and which order would satisfy a set of rules (`solve_order`). Each
takes the VFS the index belongs to, or, for the solver, only the index.

{% callout(kind="note", title="Outside the promoted surface") %}
The crate root re-exports none of the types on this page. They are public in
`vfstool_lib::analysis` and the methods are public on `LayerIndex`, but the stable 1.0 surface is
what the crate root re-exports, and `solve_order` takes its request and returns its result in
[`experimental::solve`](@/docs/api/experimental.md) types. The command line uses none of it.
{% end %}

The current winner of each key is the provider the VFS resolves it to, found by its place in the
key's provider stack, so a source that provides nothing, or that `remove_source` emptied, does not
throw the count off.

## Reordering

```rust
use std::{fs, path::PathBuf};

use vfstool_lib::VFS;
use vfstool_lib::analysis::{
    HeuristicCondition, ImpactHeuristic, ImpactProfile, ReorderOp, RiskLevel, SimOpts,
};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-simulate-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    let hd = root.join("HD Rocks");
    for (dir, files) in [
        (&base, &["textures/tx_rock.dds", "meshes/rock.nif"][..]),
        (&patch, &["textures/tx_rock.dds", "meshes/rock.nif"][..]),
        (&hd, &["textures/tx_rock.dds"][..]),
    ] {
        for file in files {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, dir.to_string_lossy().as_bytes())?;
        }
    }

    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch, &hd], None);
    let move_patch_up = ReorderOp::MoveAfter { source: patch.clone(), after: hd.clone() };
    let opts = SimOpts {
        impact_buckets: vec!["textures/**".into(), "meshes/**".into()],
        ..SimOpts::default()
    };

    let delta = layer.simulate_with_opts(&vfs, move_patch_up.clone(), &opts)?;
    assert_eq!(delta.changed_winners, 1);
    assert_eq!(delta.unchanged_winners, 1);
    assert_eq!(delta.changed_keys_sample, [PathBuf::from("textures/tx_rock.dds")]);
    assert_eq!(delta.by_bucket[0].changed_winners, 1);
    assert_eq!(delta.by_bucket[1].changed_winners, 0);
    let patch_row = &delta.by_source_gain_loss[1];
    assert_eq!((patch_row.wins_before, patch_row.wins_after), (1, 2));

    let profile = ImpactProfile {
        heuristics: vec![ImpactHeuristic {
            name: "texture winner".into(),
            path_glob: "textures/**".into(),
            weight: 2.0,
            condition: HeuristicCondition::WinnerChanged,
        }],
    };
    let impact = layer.simulate_impact(&vfs, move_patch_up, &opts, &profile)?;
    assert_eq!(impact.overall_score, 2.0);
    assert_eq!(impact.risk_level, RiskLevel::Medium);
    assert_eq!(impact.top_risky_changes[0].reasons, ["texture winner"]);

    fs::remove_dir_all(&root)
}
```

### simulate

{{ api_signature(value="fn simulate(&self, vfs: &VFS, op: ReorderOp) -> io::Result<SimulationDelta>") }}

`simulate_with_opts(vfs, op, &SimOpts::default())`: a sample of up to 100 changed keys and no
buckets.

### simulate_with_opts

{{ api_signature(value="fn simulate_with_opts(&self, vfs: &VFS, op: ReorderOp, opts: &SimOpts) -> io::Result<SimulationDelta>") }}

Applies `op` to the order of `sources`, without changing anything, and for every key compares the
VFS's current winning source with the winner under the new order: the key's loose source that
ranks highest in it, or, when only archives provide the key, the highest archive. A reorder never
lifts an archive above a loose file, as OpenMW never lets one win over a loose file; only an
archive that already outranks the loose files, pushed on top with `push_archive`, keeps winning
while the new order leaves it above them. Nothing is read from disk.

Sources in `op` are matched by exact path against `sources`, the first match if two share a path.
Errors, all `InvalidInput`:

| `op` | Message |
|---|---|
| `Swap` | `swap source A not found`, `swap source B not found` |
| `MoveBefore` | `move source not found`, `before source not found` |
| `MoveAfter` | `move source not found`, `after source not found` |
| `FullOrder` | `full-order path count does not match source count`, `unknown source in full order: PATH`, `duplicate source in full order: PATH` |

### simulate_impact

{{ api_signature(value="fn simulate_impact(&self, vfs: &VFS, op: ReorderOp, opts: &SimOpts, profile: &ImpactProfile) -> io::Result<ImpactReport>") }}

Scores the keys a reorder would change. It simulates with no sample limit; then, for each changed
key, adds the `weight` of every heuristic whose `path_glob` matches the key and whose condition
holds. Keys that score 0 are dropped.

`WinnerChanged` always holds for a changed key. `WinnerChangedAndSemanticBehaviorChanging` holds
when the new winner's content, compared with the current winner's by
[`analyze_pair`](@/docs/api/semantic.md), is `BehaviorChanging`. When any heuristic uses it, both
contents are read for every key whose winner would change, archive entries included, and a read
error is an error.

The overall score is the sum over all scored keys; `risk_level` is `Low` at 0, `Medium` below 5,
`High` below 15, and `Critical` from 15 up. `top_risky_changes` holds up to 100 keys, highest score
first, then by key. `by_bucket` sums the scores of the scored keys that match each of
`opts.impact_buckets`. Globs are [path globs](@/docs/api/keys.md).

## ReorderOp

{{ api_signature(value="enum ReorderOp") }}

A change to the load order, naming sources by path. `Debug`, `Clone`.

| Variant | Result |
|---|---|
| `Swap(PathBuf, PathBuf)` | The two sources trade places |
| `MoveBefore { source: PathBuf, before: PathBuf }` | `source` is taken out and put immediately below `before` |
| `MoveAfter { source: PathBuf, after: PathBuf }` | `source` is taken out and put immediately above `after` |
| `FullOrder(Vec<PathBuf>)` | Exactly this order, lowest priority first: every source once |

Below means lower priority, earlier in the list.

## SimOpts

{{ api_signature(value="struct SimOpts") }}

`Debug`, `Clone`, `Default`.

| Field | Default | Meaning |
|---|---|---|
| `sample_limit: usize` | 100 | How many changed keys `changed_keys_sample` keeps |
| `impact_buckets: Vec<String>` | empty | Path globs, such as `textures/**`, to total changes under |

## SimulationDelta

{{ api_signature(value="struct SimulationDelta") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `changed_winners: usize` | Keys whose winning source would change |
| `unchanged_winners: usize` | Keys whose winning source would not |
| `by_source_gain_loss: Vec<SourceDelta>` | One row per source, in `sources` order |
| `by_bucket: Vec<BucketDelta>` | One row per bucket, in the order given |
| `changed_keys_sample: Vec<PathBuf>` | The first `sample_limit` changed keys, sorted by their bytes |

## SourceDelta

{{ api_signature(value="struct SourceDelta") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `source: PathBuf` | The source's path |
| `wins_before: usize` | Keys it wins now |
| `wins_after: usize` | Keys it would win |

## BucketDelta

{{ api_signature(value="struct BucketDelta") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `bucket: String` | The glob |
| `changed_winners: usize` | Changed keys it matches |

## ImpactProfile

{{ api_signature(value="struct ImpactProfile") }}

The rules `simulate_impact` scores with. `Debug`, `Clone`; `Serialize` and `Deserialize` with
`serialize`, so a profile can live in a file.

| Field | Meaning |
|---|---|
| `heuristics: Vec<ImpactHeuristic>` | Every rule; all that match a key add up |

## ImpactHeuristic

{{ api_signature(value="struct ImpactHeuristic") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `name: String` | Listed in `reasons` when the rule scores |
| `path_glob: String` | Which keys it applies to |
| `weight: f32` | What it adds to a key's score |
| `condition: HeuristicCondition` | When it applies |

## HeuristicCondition

{{ api_signature(value="enum HeuristicCondition") }}

`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`; `Serialize` and `Deserialize` with `serialize`, as
`"winner_changed"` and `"winner_changed_and_semantic_behavior_changing"`.

| Variant | Applies when |
|---|---|
| `WinnerChanged` | The key's winner would change |
| `WinnerChangedAndSemanticBehaviorChanging` | It would change, and the new content differs from the old in a way `analyze_pair` calls `BehaviorChanging` |

## ImpactReport

{{ api_signature(value="struct ImpactReport") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `overall_score: f32` | The sum of every scored key |
| `risk_level: RiskLevel` | The overall score, banded |
| `by_bucket: Vec<BucketImpact>` | One row per `impact_buckets` glob |
| `top_risky_changes: Vec<RiskyChange>` | Up to 100 scored keys, highest first |

## BucketImpact

{{ api_signature(value="struct BucketImpact") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `bucket: String` | The glob |
| `score: f32` | The summed score of the scored keys it matches |

## RiskyChange

{{ api_signature(value="struct RiskyChange") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `key: PathBuf` | The key |
| `score: f32` | Its summed weight |
| `reasons: Vec<String>` | The names of the heuristics that scored it, in profile order |

## RiskLevel

{{ api_signature(value="enum RiskLevel") }}

`Low`, `Medium`, `High`, `Critical`, in that order. `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`,
`Ord`, `PartialOrd`; `Serialize` and `Deserialize` with `serialize`, in lowercase.

## Adding a directory

{{ api_signature(value="fn plan_candidate_directory(&self, vfs: &VFS, candidate_dir: &Path, opts: CandidatePlanOpts) -> io::Result<CandidatePlan>") }}

What installing `candidate_dir` above everything else would do, from
[`VFS::diff_directory`](@/docs/api/conflicts.md), sorted. Every key the directory adds, and every
key it already shares with the VFS: each of those would be displaced, since the new directory would
win it. When the directory has two files for one key, one of them, unspecified, is reported.

With `opts.include_semantic`, the candidate file and the current winner are both read and hashed,
and `semantic_differs` says whether the bytes differ. It is a byte comparison, not the semantic
analysis. A file that cannot be read is an error.

```rust
use std::fs;

use vfstool_lib::VFS;
use vfstool_lib::analysis::CandidatePlanOpts;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-candidate-example");
    let data = root.join("Data Files");
    let candidate = root.join("New Mod");
    fs::create_dir_all(data.join("textures"))?;
    fs::create_dir_all(candidate.join("textures"))?;
    fs::write(data.join("textures/tx_rock.dds"), b"old")?;
    fs::write(data.join("textures/tx_moss.dds"), b"moss")?;
    fs::write(candidate.join("textures/tx_rock.dds"), b"new")?;
    fs::write(candidate.join("textures/tx_moss.dds"), b"moss")?;
    fs::write(candidate.join("textures/tx_sand.dds"), b"sand")?;

    let (vfs, layer) = VFS::from_directories_with_layer_index([&data], None);
    let plan = layer.plan_candidate_directory(&vfs, &candidate, CandidatePlanOpts::default())?;

    assert_eq!(plan.summary.additions, 1);
    assert_eq!(plan.summary.conflicts, 2);
    assert_eq!(plan.conflicts[0].current_winner_source, data);
    assert_eq!(plan.conflicts[0].semantic_differs, Some(false)); // tx_moss.dds, same bytes
    assert_eq!(plan.conflicts[1].semantic_differs, Some(true)); // tx_rock.dds

    fs::remove_dir_all(&root)
}
```

## CandidatePlanOpts

{{ api_signature(value="struct CandidatePlanOpts") }}

`Debug`, `Clone`, `Copy`, `Default`.

| Field | Default | Meaning |
|---|---|---|
| `include_semantic: bool` | `true` | Hash both sides of every conflict and fill in `semantic_differs` |

## CandidatePlan

{{ api_signature(value="struct CandidatePlan") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `additions: Vec<PathBuf>` | Keys the directory would add, sorted |
| `conflicts: Vec<CandidateConflict>` | Keys it shares with the VFS, sorted |
| `displaced_winners: Vec<PathBuf>` | The keys whose current winner it would replace: every conflict's key, sorted |
| `summary: CandidatePlanSummary` | The three counts |

## CandidateConflict

{{ api_signature(value="struct CandidateConflict") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `key: PathBuf` | The key |
| `current_winner_source: PathBuf` | The path of the source that wins it now; empty when the index cannot say |
| `candidate_file: PathBuf` | The file in the candidate directory |
| `semantic_differs: Option<bool>` | Whether the two files' bytes differ; `None` without `include_semantic` |
| `risk: Option<RiskLevel>` | Always `None` in 1.0.0 |

## CandidatePlanSummary

{{ api_signature(value="struct CandidatePlanSummary") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `additions: usize` | Length of `additions` |
| `conflicts: usize` | Length of `conflicts` |
| `displaced_winners: usize` | Length of `displaced_winners` |

## Solving an order

{{ api_signature(value="fn solve_order(&self, request: &SolveRequest) -> io::Result<SolveResult>") }}

Finds a load order that satisfies `request.constraints`, moving as little as possible from
`request.current_order`, or from `sources` order when that is empty. It works on the index alone:
no VFS, and nothing is read. The request, the constraints and the result are
[`experimental::solve`](@/docs/api/experimental.md) types, which that page lists in full.

The solver's winner for a key is its highest-ranked loose source, or, when only archives provide
it, the highest-ranked archive: archives stay below loose files, as in a VFS.

1. It orders the sources so every `SourceBefore` and `SourceAfter` holds, keeping the current
   order wherever the constraints leave a choice. If they form a cycle, the result is
   `Unsatisfiable`, with `order` `None` and one violation per precedence constraint, whether or not
   it is part of the cycle, each reading `precedence constraints contain a cycle`.
2. It checks every constraint against that order. If some fail, and the failure is not certain (a
   `WinnerMustBe` whose glob matches no key, or a key no allowed source provides), it moves one
   source at a time, keeping the precedence constraints, taking each step that leaves fewer
   violations or, as many, fewer moved positions, for up to the number of sources squared steps.
3. If that does not satisfy everything and there are at most 9 sources, it tries every order the
   precedence constraints allow and keeps the one that satisfies everything with the fewest moved
   positions.

`Satisfiable` comes with `order`, lowest priority first. `Unsatisfiable` comes with the violations
of the best order found and no `order`. With more than 9 sources, an order can exist that the local
search does not find. `diagnostics.move_count` is the number of positions where the result differs
from the current order, and `diagnostics.changed_winners` the number of keys whose winner, by the
solver's rule, it changes.

Errors, all `InvalidInput`: two sources in the index with the same path
(`duplicate source path: PATH`); a `current_order` that is not every source exactly once
(`current_order length does not match source count`, `unknown source in current_order: PATH`,
`duplicate source in current_order: PATH`); a constraint naming a source the index does not have
(`unknown source in constraint: PATH`).

```rust
use std::fs;

use vfstool_lib::VFS;
use vfstool_lib::experimental::solve::{
    OrderConstraint, SolveObjective, SolveRequest, SolveStatus,
};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-solve-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    let hd = root.join("HD Rocks");
    for dir in [&base, &patch, &hd] {
        fs::create_dir_all(dir.join("textures"))?;
        fs::write(dir.join("textures/tx_rock.dds"), dir.to_string_lossy().as_bytes())?;
    }

    let (_vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch, &hd], None);
    let request = SolveRequest {
        current_order: Vec::new(),
        constraints: vec![OrderConstraint::WinnerMustBe {
            path_glob: "textures/**".into(),
            source_glob: "**/rock retexture".into(),
        }],
        objective: SolveObjective::MinMovesFromCurrent,
    };
    let result = layer.solve_order(&request)?;

    assert_eq!(result.status, SolveStatus::Satisfiable);
    assert_eq!(result.order, Some(vec![base.clone(), hd.clone(), patch.clone()]));
    assert_eq!(result.diagnostics.move_count, 2);
    assert_eq!(result.diagnostics.changed_winners, 1);

    fs::remove_dir_all(&root)
}
```
