# vfstool_lib

`vfstool_lib` is a reimplementation of OpenMW's virtual file system (VFS). It provides tools for working with directory structures, archive files, and file metadata — ideal for applications that handle complex mod loadouts. It does not depend on OpenMW or any particular game.

---

## Features

- **Virtual File System (VFS)**: Build from ordered data directories. Later directories win (matching OpenMW `data=` semantics). Loose files always beat archive files.
- **Provider and conflict analysis**: `VFS` stores provider stacks and a cached resolved winner map;
  `LayerIndex` is its provenance projection and `ConflictIndex` is a derived conflict projection for
  override/overridden reports.
- **Archive support**: BSA/BA2 (Morrowind, Oblivion, Skyrim, Fallout 4) via `dream_archive` (`beth-archives` feature). ZIP/PK3 via the `zip` crate (`zip` feature).
- **Serialization**: JSON, YAML, TOML output via `serde` (`serialize` feature).
- **Semantic JSON/TOML analysis**: Structured JSON/TOML comparisons require the `serialize` feature;
  without it those formats are reported as unknown semantic deltas.
- **Parallel processing**: Directory walks and hash operations use `rayon`.
- **MO2-style runner support**: `run_setup` / `run_finalize` for dump-run-collect workflows.
- **Mutable VFS**: `VFS` is provider-aware. Winner-only operations and stack-preserving operations
  are deliberately named differently, because APIs should not lie for sport.

---

## Installation

```toml
[dependencies]
vfstool_lib = "1.0"
```

With archive and serialization support:

```toml
[dependencies]
vfstool_lib = { version = "1.0", features = ["beth-archives", "zip", "serialize"] }
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

The analysis model has one source of truth. `VFS` stores every provider for every normalized key in
low-to-high priority order, plus a cached resolved-winner map for fast lookup/materialization.
`LayerIndex` is rebuilt from those provider stacks for provenance workflows; `ConflictIndex` is
intentionally narrower and derived when callers need MO2-style override/overridden sets or
source-to-source diffs. If a report needs provider chains, use `VFS`/`LayerIndex`; if it needs only
conflict arrows, use `ConflictIndex`. Two separate truths would be exciting, in the same way an FBO
completeness bug is exciting.

```rust
use vfstool_lib::{ConflictIndex, LayerIndex, SourceKind, SourceMeta, VFS};
use std::path::{Path, PathBuf};

let (vfs, ci) = VFS::from_directories_with_conflict_index(
    vec!["path/to/base", "path/to/mod"],
    None,
);

let provider_chain = vfs.provider_records_for(Path::new("textures/foo.dds"));
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

Requires the `serialize` feature.

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
cargo run -p vfstool_lib --example provider_stack_vfs
```

These examples intentionally use temporary fixtures rather than a real OpenMW install, so they are
safe starting points for application code.

### Mutating VFS contents

`VFS` stores provider stacks low-to-high priority. The resolved winner for a key is always the last
provider in that stack, cached in the winner map used by `get_file()` and materialization.

Use the names to choose the semantics you actually want:

- `set_winner_file` / `set_winner_loose_file`: replace the whole provider stack for one key with a
  single resolved winner.
- `push_provider`: add a higher-priority provider without discarding lower-priority providers.
- `remove_winner`: remove only the current winner and reveal the next lower-priority provider.
- `remove_resolved_file`: remove the resolved key entirely, discarding all providers for that key.

```rust
use vfstool_lib::{VFS, VfsFile};

let mut vfs = VFS::new();
vfs.set_winner_file("textures/foo.dds", VfsFile::from("/mods/high/textures/foo.dds"));
let removed = vfs.remove_resolved_file("Textures/Foo.dds");
assert!(removed.is_some());
```

Provider-preserving mutation uses the same `VFS` type:

```rust
use vfstool_lib::VFS;

let mut vfs = VFS::from_directories(["/mods/base", "/mods/high"], None);
vfs.remove_winner("textures/foo.dds");
# let _ = vfs;
```

With the `beth-archives` or `zip` feature enabled, `VFS::from_directories` resolves archive names
through the loose directory files and inserts archive providers below all loose providers, matching
OpenMW's loose-over-archive rule. Manual `push_archive` is different: it pushes that archive as a new
highest-priority provider source, because that is what "push" means.

Archive entries that normalize to the same VFS key are preserved in provider reports and case
collision reports. The resolved winner still follows provider order; reporting does not silently turn
two in-archive spellings into one entry just because a map was convenient.

```rust,no_run
#[cfg(any(feature = "beth-archives", feature = "zip"))]
# {
use vfstool_lib::VFS;

let vfs = VFS::from_directories(
    ["/games/Morrowind/Data Files"],
    Some(vec!["Morrowind.bsa"]),
);
# let _ = vfs;
# }
# Ok::<(), std::io::Error>(())
```

`VFS` source removal uses lexical path equality. Use the same source path representation for
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
`VfsFile`, `VfsProvider`, conflict/report types, semantic analyzer/report types, path helpers,
lock/drift types, runner helpers, and serialization helpers. The `semantic` module is public and
stable, but still deliberately modest: it can classify JSON/TOML/INI/text-ish differences, not solve
every mod conflict in existence. JSON and TOML structural comparison require the `serialize` feature;
without it those deltas are unknown rather than parsed. The `experimental` namespace remains public
for policy, solver, and knowledge-base workflows, but it is not promoted or stable API.

### Breaking API changes in 1.0

- VFS keys are now byte-first normalized resource keys. Public key-facing APIs use or accept
  `dream_path::NormalizedPath` through `vfstool_lib::NormalizedPath` / `VfsKeyInput` rather than
  treating VFS keys as host filesystem `PathBuf`s. Real filesystem paths still use `Path`/`PathBuf`.
- Bethesda archive loading moved from the old direct BA2/BSA plumbing to `dream_archive`; enable it
  with `beth-archives`. The old `bsa` Cargo feature name was removed because it claimed to be one
  archive format while quietly enabling two. That sort of thing is how rendering options end up
  disabling shadows without disabling the shadow pass.
- With `serialize` enabled, `serde`, `serde_json`, `serde_yaml`, and `toml` are re-exported from
  `vfstool_lib` so applications can use the same serialization stack as the library instead of
  pinning duplicate parser versions.
- ZIP/PK3 support is intentionally narrower: `zip` is built without default features and currently
  supports stored/deflated and LZMA-compressed entries. AES, bzip2, PPMd, deflate64, and zstd are not
  pulled in unless we deliberately decide they are worth the dependency cost.
- `MutableVfs` was removed before 1.0. `VFS` now owns provider stacks directly, so there is no second
  VFS implementation to drift out of sync. The old distinction is represented by explicit method
  names instead of a second type.

---

## Feature flags

| Flag | Description |
|------|-------------|
| `beth-archives` | BSA/BA2 archive support (Morrowind, Oblivion, Skyrim, Fallout 4) |
| `zip` | ZIP/PK3 archive support |
| `serialize` | JSON/YAML/TOML output via serde |
| `lua` | Embedded `mlua` bindings for the promoted stable API surface; see [`docs/lua.md`](docs/lua.md) |
| `standalone-lua` | Enables `lua` with vendored LuaJIT for embedded standalone tools; not a `cdylib` Lua module |

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

Licensed under the [GNU General Public License v3.0](../LICENSE).
