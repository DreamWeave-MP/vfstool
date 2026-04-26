# vfstool_lib

`vfstool_lib` is a reimplementation of OpenMW's virtual file system (VFS). It provides tools for working with directory structures, archive files, and file metadata — ideal for applications that handle complex mod loadouts. It does not depend on OpenMW or any particular game.

---

## Features

- **Virtual File System (VFS)**: Build from ordered data directories. Later directories win (matching OpenMW `data=` semantics). Loose files always beat archive files.
- **Provider and conflict analysis**: `LayerIndex` stores canonical provider chains; `VFS`
  resolves winners from that index; `ConflictIndex` is a derived conflict projection for
  override/overridden reports.
- **Archive support**: BSA/BA2 (Morrowind, Oblivion, Skyrim, Fallout 4) via the `ba2` crate (`bsa` feature). ZIP/PK3 via the `zip` crate (`zip` feature).
- **Serialization**: JSON, YAML, TOML output via `serde` (`serialize` feature).
- **Semantic JSON/TOML analysis**: Structured JSON/TOML comparisons require the `serialize` feature;
  without it those formats are reported as unknown semantic deltas.
- **Parallel processing**: Directory walks and hash operations use `rayon`.
- **MO2-style runner support**: `run_setup` / `run_finalize` for dump-run-collect workflows.
- **Mutable views**: Winner-only mutation on `VFS`, plus provider-aware loose/archive mutation with `MutableVfs`.

---

## Installation

```toml
[dependencies]
vfstool_lib = "1.0"
```

With archive and serialization support:

```toml
[dependencies]
vfstool_lib = { version = "1.0", features = ["bsa", "zip", "serialize"] }
```

---

## Usage

### Basic example

```rust
use vfstool_lib::VFS;
use std::path::PathBuf;

fn main() {
    let search_dirs = vec![
        PathBuf::from("path/to/base"),
        PathBuf::from("path/to/mod"),   // higher priority
    ];
    let archive_list = Some(vec!["Morrowind.bsa"]);

    let vfs = VFS::from_directories(search_dirs, archive_list);

    for (key, file) in vfs.iter() {
        println!("{key:?} → {file:?}");
    }
}
```

### Conflict analysis

The analysis model has one source of truth. `LayerIndex` records every provider for every
normalized VFS key in low-to-high priority order. `VFS` owns a `LayerIndex` and the resolved winner
map; queries such as `explain`, duplicate keys, source contributions, archives, and case collisions
are projections over that owned provider index. `ConflictIndex` is intentionally narrower: it is
derived from `LayerIndex` when callers need MO2-style override/overridden sets or source-to-source
diffs. If a report needs provider chains, use `LayerIndex`/`VFS`; if it needs only conflict arrows,
use `ConflictIndex`. Two separate truths would be exciting, in the same way an FBO completeness bug
is exciting.

```rust
use vfstool_lib::{ConflictIndex, LayerIndex, SourceKind, SourceMeta, VFS};
use std::path::{Path, PathBuf};

let (vfs, ci) = VFS::from_directories_with_conflict_index(
    vec!["path/to/base", "path/to/mod"],
    None,
);

let provider_chain = vfs.providers_for(Path::new("textures/foo.dds"));
let duplicate_keys = vfs.layer_index().duplicate_keys();

let report = ci.conflicts_report(true);  // use_relative = true
for entry in &report.sources {
    println!("{}: {} overrides, {} overridden",
        entry.path.display(),
        entry.overrides.len(),
        entry.overridden_by.len());
}

let layer = LayerIndex::from_file_lists([(
    SourceMeta { path: PathBuf::from("path/to/base"), kind: SourceKind::LooseDir },
    vec![PathBuf::from("textures/foo.dds")],
)]);
let conflicts = ConflictIndex::from_layer_index(&layer);
let contributions = layer.source_contributions();
# let _ = (vfs, provider_chain, duplicate_keys, conflicts, contributions);
```

### Serialization

```rust
use vfstool_lib::{SerializeType, VFS};

let vfs = VFS::from_directories(vec!["path/to/data"], None);
let tree = vfs.tree(false);
let json = vfs.serialize_from_tree(&tree, SerializeType::Json).unwrap();
println!("{json}");
```

### Runnable examples

The crate includes small examples that compile against the public 1.0 API:

```bash
cargo run -p vfstool_lib --example basic_vfs
cargo run -p vfstool_lib --example provider_reports
cargo run -p vfstool_lib --example semantic_analysis
cargo run -p vfstool_lib --example mutable_vfs
```

These examples intentionally use temporary fixtures rather than a real OpenMW install, so they are
safe starting points for application code.

### Mutating VFS contents

`VFS` mutators edit the materialized winner map directly. Removing a winner removes that key from
the resolved view; it does not reveal lower-priority providers.

```rust
use vfstool_lib::{VFS, VfsFile};

let mut vfs = VFS::new();
vfs.insert_file("textures/foo.dds", VfsFile::from("/mods/high/textures/foo.dds"));
let removed = vfs.remove_file("Textures/Foo.dds");
assert!(removed.is_some());
```

Use `MutableVfs` when provider stacks matter. Providers are stored low-to-high priority, so removing
the current winner reveals the next lower-priority provider if one exists. With the `bsa` or `zip`
feature enabled, `MutableVfs::from_directories_with_archives` resolves archive names through the
loose directory files and inserts archive providers below all loose providers, matching OpenMW's
loose-over-archive rule.

```rust
use vfstool_lib::MutableVfs;

let mut mutable = MutableVfs::from_directories(["/mods/base", "/mods/high"])?;
mutable.remove_winner("textures/foo.dds");
let resolved = mutable.to_vfs();
# Ok::<(), std::io::Error>(())
```

`MutableVfs::to_vfs()` converts the mutable provider stack into a resolved winner view. It preserves
the current winners, not the full mutable provider history. If later removal should reveal lower
priority providers, keep using `MutableVfs`; converting to `VFS` is the point where that behaviour is
intentionally flattened. A conversion that says it preserves stacks while quietly dropping them would
be worse, so this one says what it does.

```rust,no_run
#[cfg(any(feature = "bsa", feature = "zip"))]
# {
use vfstool_lib::MutableVfs;

let mutable = MutableVfs::from_directories_with_archives(
    ["/games/Morrowind/Data Files"],
    &["Morrowind.bsa"],
)?;
# let _ = mutable;
# }
# Ok::<(), std::io::Error>(())
```

`MutableVfs` source removal uses lexical path equality. Use the same source path representation for
removal that you used when inserting/building providers.

### Runner hardlink behavior

`run_setup` can populate the merged directory with hardlinks. This is intentional for speed and disk
usage, but tools that edit merged files in place may mutate the original loose source files through
those hardlinks. Use copy mode when running tools that are not hardlink-safe.

`run_setup` creates the merged directory if needed. If it already exists, it removes that directory
recursively before rebuilding it so child tools see only the current VFS contents. Pass a dedicated
scratch directory, not a directory containing user data.

---

## 1.0 API surface

The stable 1.0 API is the top-level re-exported surface from `vfstool_lib`, including `VFS`,
`VfsFile`, `MutableVfs`, conflict/report types, semantic analyzer/report types, path helpers,
lock/drift types, runner helpers, and serialization helpers. The `semantic` module is public and
stable, but still deliberately modest: it can classify JSON/TOML/INI/text-ish differences, not solve
every mod conflict in existence. JSON and TOML structural comparison require the `serialize` feature;
without it those deltas are unknown rather than parsed. The `experimental` namespace remains public
for policy, solver, and knowledge-base workflows, but it is not promoted or stable API.

---

## Feature flags

| Flag | Description |
|------|-------------|
| `bsa` | BSA/BA2 archive support (Morrowind, Oblivion, Skyrim, Fallout 4) |
| `zip` | ZIP/PK3 archive support |
| `serialize` | JSON/YAML/TOML output via serde |

---

## Benchmarks

The library benchmark suite covers common VFS operations and several release-sensitive large-loadout
paths:

```bash
cargo bench -p vfstool_lib --bench vfs_benchmarks
cargo bench -p vfstool_lib --bench vfs_benchmarks --features zip,serialize
```

The suite includes normalization, construction, lookup, tree building, diffing, conflict indexing,
serialization, ZIP materialization, semantic conflict analysis, dump/run setup, sparse tracked
finalization, and high-conflict-density load orders. BSA/BA2 performance still depends on real archive
fixtures; if you are optimizing that path, measure with representative game archives rather than
pretending a synthetic ZIP is the same thing. It is not.

---

## License

Dual-licensed under [MIT](../LICENSE-MIT) or [Apache 2.0](../LICENSE-APACHE) at your option.
