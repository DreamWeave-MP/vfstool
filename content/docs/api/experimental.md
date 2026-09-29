+++
title = "experimental"
description = "vfstool_lib::experimental: policy rules checked against a VFS, the load-order solver's request and result types, and a file-backed store of known conflict outcomes."
weight = 140

[extra]
kind = "api"
+++

{% callout(kind="warning", title="Public, not stable") %}
Everything under `vfstool_lib::experimental` can change or disappear in any release, minor or
patch. It is public so tools can try it, and documented here so they know what they are trying. The
command line uses none of it.
{% end %}

Three modules, each re-exporting a private one whole:

| Module | Holds |
|---|---|
| `experimental::policy` | Rules a VFS's winners must meet, and their evaluation |
| `experimental::solve` | The request and result types of [`LayerIndex::solve_order`](@/docs/api/planning.md) |
| `experimental::kb` | Fingerprints of conflicts, known outcomes for them, and a YAML file to keep them in |

## Globs

Policy rules and solver constraints match keys and source paths with the crate's globs, as
[`path_glob_matches`](@/docs/api/keys.md) does:

| Pattern | Matches |
|---|---|
| `*` | Any run of characters within one path component |
| `?` | One character within a component |
| `**/` | Zero or more whole directories |
| `**` elsewhere | Anything, across components |

Both the pattern and the path have `\` turned into `/` and ASCII letters lowercased first, and the
whole path must match. Every other character is literal, `[` and `{` included. A pattern that
cannot be compiled is an `InvalidInput` error: `invalid path_glob 'PATTERN': REASON`, or
`source_glob`.

## policy

```rust
use std::fs;

use vfstool_lib::experimental::policy::{Policy, Rule};
use vfstool_lib::{SourceKind, VFS};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-policy-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    fs::create_dir_all(base.join("textures"))?;
    fs::create_dir_all(base.join("meshes"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("textures/tx_rock.dds"), b"original")?;
    fs::write(base.join("meshes/rock.nif"), b"mesh")?;
    fs::write(patch.join("textures/tx_rock.dds"), b"retextured")?;

    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch], None);
    let policy = Policy {
        rules: vec![
            Rule::WinnerMustMatch { path_glob: "textures/**".into(), source_glob: "**/rock retexture".into() },
            Rule::MustBeUnique { path_glob: "**/*.dds".into() },
            Rule::MustExist { path_glob: "icons/**".into() },
            Rule::WinnerKindMustBe { path_glob: "**".into(), kind: SourceKind::LooseDir },
        ],
    };
    let result = policy.evaluate(&layer, &vfs)?;

    let found: Vec<_> = result.violations.iter().map(|violation| (violation.rule.as_str(), violation.message.as_str())).collect();
    assert_eq!(found, [
        ("must_be_unique", "key has 2 providers"),
        ("must_exist", "no key matched 'icons/**'"),
    ]);

    fs::remove_dir_all(&root)
}
```

{{ api_signature(value="struct Policy") }}

`Debug`, `Clone`. No serialization: a policy is built in code.

| Field | Meaning |
|---|---|
| `rules: Vec<Rule>` | Checked in order; every violation of every rule is reported |

{{ api_signature(value="fn evaluate(&self, index: &LayerIndex, vfs: &VFS) -> io::Result<PolicyResult>") }}

Checks every rule against the keys that resolve in `vfs`. Winners come from
[`provenance`](@/docs/api/layer-index.md), so a key whose winner `index` and `vfs` disagree on is
skipped by the winner rules; provider counts come from `index`. Violations are sorted by rule name,
then key, then message. Errors: a glob that cannot be compiled.

{{ api_signature(value="enum Rule") }}

`Debug`, `Clone`.

| Variant | Violated when | `rule` | `message` |
|---|---|---|---|
| `WinnerMustMatch { path_glob: String, source_glob: String }` | A matching key's winning source path does not match `source_glob` | `winner_must_match` | `winner 'SOURCE' does not match source glob 'GLOB'` |
| `WinnerMustNotMatch { path_glob: String, source_glob: String }` | A matching key's winning source path matches it | `winner_must_not_match` | `winner 'SOURCE' matches forbidden source glob 'GLOB'` |
| `MustExist { path_glob: String }` | No key matches. The one violation without a key | `must_exist` | `no key matched 'GLOB'` |
| `MustBeUnique { path_glob: String }` | A matching key has more than one provider | `must_be_unique` | `key has N providers` |
| `WinnerKindMustBe { path_glob: String, kind: SourceKind }` | A matching key's winner is the other kind | `winner_kind_must_be` | `winner kind mismatch: expected LooseDir, got Archive` |
| `MaxOverrideDepth { path_glob: String, max: usize }` | A matching key has more than `max` providers | `max_override_depth` | `provider_count N exceeds max MAX` |

Providers are counted per occurrence, as in [`LayerIndex`](@/docs/api/layer-index.md). Each rule
violated by several keys gives one violation per key.

{{ api_signature(value="enum Severity") }}

One variant, `Error`. `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`; `Serialize` with `serialize`,
as `"Error"`.

{{ api_signature(value="struct Violation") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `rule: String` | The rule's name, from the table |
| `key: Option<PathBuf>` | The key that broke it; `None` for `must_exist` |
| `message: String` | What is wrong, from the table |
| `severity: Severity` | Always `Error` |

{{ api_signature(value="struct PolicyResult") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `violations: Vec<Violation>` | Every violation, sorted; empty when the policy holds |

## solve

[Solving an order](@/docs/api/planning.md) has the algorithm and an example. All of these are
`Debug` and `Clone`, and `Serialize` and `Deserialize` with `serialize`, with variant names as
written: `"SourceBefore"`, `"MinMovesFromCurrent"`, `"Satisfiable"`.

{{ api_signature(value="struct SolveRequest") }}

| Field | Meaning |
|---|---|
| `current_order: Vec<PathBuf>` | Every source path in the index once, lowest priority first; or empty, for the index's own order |
| `constraints: Vec<OrderConstraint>` | What the result must satisfy |
| `objective: SolveObjective` | What to prefer among satisfying orders |

{{ api_signature(value="enum OrderConstraint") }}

`PartialEq`, `Eq` as well. Sources are named by exact path.

| Variant | Requires | Violation `message` |
|---|---|---|
| `SourceBefore { a: PathBuf, b: PathBuf }` | `a` lower in the order than `b` | `source 'A' is not before 'B'` |
| `SourceAfter { a: PathBuf, b: PathBuf }` | `a` higher than `b` | `source 'A' is not after 'B'` |
| `WinnerMustBe { path_glob: String, source_glob: String }` | Every key matching `path_glob` won by a source matching `source_glob`, by the solver's winner rule | `winner for matching key does not satisfy source glob 'GLOB'`, one per key, or `winner_must_be matched no keys for glob 'GLOB'` |

{{ api_signature(value="enum SolveObjective") }}

`Copy`, `PartialEq`, `Eq` as well. One variant, `MinMovesFromCurrent`: change as few positions as
possible.

{{ api_signature(value="struct SolveResult") }}

| Field | Meaning |
|---|---|
| `status: SolveStatus` | Whether an order was found |
| `order: Option<Vec<PathBuf>>` | The order, lowest priority first, when `Satisfiable` |
| `diagnostics: SolveDiagnostics` | Violations and costs |

{{ api_signature(value="enum SolveStatus") }}

`Copy`, `PartialEq`, `Eq` as well.

| Variant | Meaning |
|---|---|
| `Satisfiable` | `order` meets every constraint |
| `Unsatisfiable` | No order was found. With more than 9 sources, one may still exist |

{{ api_signature(value="struct SolveDiagnostics") }}

| Field | Meaning |
|---|---|
| `violated_constraints: Vec<ConstraintViolation>` | What the best order found still breaks; empty when `Satisfiable` |
| `move_count: usize` | Positions where that order differs from the current one |
| `changed_winners: usize` | Keys whose winner it changes |

{{ api_signature(value="struct ConstraintViolation") }}

| Field | Meaning |
|---|---|
| `constraint_index: usize` | The constraint's position in `constraints`, from 0 |
| `message: String` | What is wrong |
| `sample_key: Option<PathBuf>` | For `WinnerMustBe`, the key whose winner is wrong |

## kb

A knowledge base: conflicts someone has looked at, and what they decided. A fingerprint names a
conflict by the two sources, the key and, optionally, both contents' hashes; an entry attaches an
outcome to it.

```rust
use std::fs;

use vfstool_lib::experimental::kb::{
    KnowledgeEntry, KnowledgeStore, KnownOutcome, LocalKnowledgeStore, conflict_fingerprints_from_report,
};
use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-kb-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    for dir in [&base, &patch] {
        fs::create_dir_all(dir.join("textures"))?;
        fs::write(dir.join("textures/tx_rock.dds"), dir.to_string_lossy().as_bytes())?;
    }

    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch], None);
    let report = layer.semantic_conflicts(&vfs)?;
    let fingerprint = conflict_fingerprints_from_report(&report).remove(0);
    assert_eq!(fingerprint.low_source, base);
    assert_eq!(fingerprint.high_source, patch);

    let mut store = LocalKnowledgeStore::new(root.join("knowledge.yaml"));
    store.upsert(KnowledgeEntry {
        fingerprint: fingerprint.clone(),
        outcome: KnownOutcome::SafeIntentionalOverride,
        confidence: 1.0,
        notes: "The retexture is meant to win.".into(),
    })?;

    let known = store.lookup(&fingerprint)?;
    assert_eq!(known[0].outcome, KnownOutcome::SafeIntentionalOverride);
    assert!(fs::read_to_string(root.join("knowledge.yaml"))?.contains("safe_intentional_override"));

    fs::remove_dir_all(&root)
}
```

{{ api_signature(value="struct ConflictFingerprint") }}

`Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`; `Serialize` and `Deserialize` with `serialize`. Two
fingerprints are the same only when all five fields are equal, hashes included.

| Field | Meaning |
|---|---|
| `low_source: PathBuf` | The overridden source |
| `high_source: PathBuf` | The source that wins |
| `key_pattern: String` | The key; nothing matches it as a pattern |
| `low_hash: Option<String>` | The overridden content's BLAKE3 hash, when known |
| `high_hash: Option<String>` | The winning content's, when known |

{{ api_signature(value="enum KnownOutcome") }}

`Debug`, `Clone`, `PartialEq`, `Eq`; `Serialize` and `Deserialize` with `serialize`, in snake case.

| Variant | Meaning |
|---|---|
| `SafeNoOp` | The conflict changes nothing that matters |
| `SafeIntentionalOverride` | The winner is meant to win |
| `RequiresManualPatch` | Someone has to merge the two |
| `KnownBreakage` | This combination is known to break |

{{ api_signature(value="struct KnowledgeEntry") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `fingerprint: ConflictFingerprint` | The conflict |
| `outcome: KnownOutcome` | The verdict |
| `confidence: f32` | Meant to be between 0.0 and 1.0; nothing checks |
| `notes: String` | Free text |

{{ api_signature(value="trait KnowledgeStore") }}

Where entries are kept. Implement it for another backend.

{{ api_signature(value="fn lookup(&self, fingerprint: &ConflictFingerprint) -> io::Result<Vec<KnowledgeEntry>>") }}

The entries whose fingerprint equals `fingerprint`.

{{ api_signature(value="fn upsert(&mut self, entry: KnowledgeEntry) -> io::Result<()>") }}

Replaces the entry with the same fingerprint, or adds this one.

{{ api_signature(value="fn all(&self) -> io::Result<Vec<KnowledgeEntry>>") }}

Every entry.

{{ api_signature(value="struct LocalKnowledgeStore") }}

A `KnowledgeStore` in one YAML file: a list of entries. No derived traits.

{{ api_signature(value="fn new(path: PathBuf) -> Self") }}

A store at `path`. Nothing is read or created until it is used. A file that does not exist is an
empty store, and so is an empty one. `upsert` reads the whole file, replaces or appends the entry,
sorts by `low_source`, `high_source` and `key_pattern`, and writes the whole file again, creating
its directory. Without the `serialize` feature, only a store whose file does not exist can be read,
and nothing can be written: those calls fail with `Unsupported`,
`YAML knowledge store 'PATH' requires the serialize feature`. A file that is not a valid store is
`InvalidData`.

{{ api_signature(value="fn conflict_fingerprints_from_report(report: &SemanticConflictReport) -> Vec<ConflictFingerprint>") }}

One fingerprint for every overridden provider in a [semantic conflict
report](@/docs/api/semantic.md): the provider as the low side, the chain's last provider as the high
side, the key as text, and both hashes as the report has them. Entries in report order, providers
lowest first.
