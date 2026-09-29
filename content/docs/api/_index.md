+++
title = "Rust API"
description = "vfstool_lib, the library vfstool is built on: its features, what is stable, and every public type and function, page by page."
template = "docs/section.html"
page_template = "docs/page.html"
sort_by = "weight"
weight = 90

[extra]
kind = "api"
hide_child_cards = true
+++

The crate `vfstool_lib` is OpenMW's virtual file system as a library: every command of the
`vfstool` program is a call into it. It builds the VFS from data directories and archives, keeps
every provider of every key, reports on them, and writes the result out. It does not read
`openmw.cfg`; the program uses [openmw-config](https://crates.io/crates/openmw-config) for that and
hands the library the directories and archive names.

```toml
[dependencies]
vfstool_lib = { version = "1", features = ["beth-archives", "zip", "serialize"] }
```

```rust
use std::fs;

use vfstool_lib::{SerializeType, VFS, serialize_value};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-index");
    let base = root.join("Data Files");
    let patch = root.join("Better Doors");
    fs::create_dir_all(base.join("Meshes/X"))?;
    fs::create_dir_all(patch.join("meshes/x"))?;
    fs::write(base.join("Meshes/X/Ex_Door.NIF"), "vanilla")?;
    fs::write(patch.join("meshes/x/ex_door.nif"), "better")?;

    // The data= directories, lowest priority first, and the fallback-archive= names.
    let vfs = VFS::from_directories([&base, &patch], Some(vec!["Morrowind.bsa"]));

    let door = vfs.get_file(r"Meshes\X\Ex_Door.NIF").unwrap();
    assert_eq!(door.path(), patch.join("meshes/x/ex_door.nif"));

    let explain = vfs.explain("meshes/x/ex_door.nif").unwrap();
    print!("{}", serialize_value(&explain, SerializeType::Yaml)?);

    fs::remove_dir_all(&root)
}
```

`Morrowind.bsa` is not in this fixture, so it is left out; a real `Data Files` would have it.

## Pages

| Page | Covers |
|---|---|
| [VFS](@/docs/api/vfs.md) | `VFS`: building one, lookups, iteration, prefix, substring and regex searches, `remaining` |
| [Changing a VFS](@/docs/api/mutation.md) | `VfsProvider`, and adding and removing providers, winner-only or stack by stack |
| [Provider reports](@/docs/api/providers.md) | `explain`, `duplicates`, `archives`, archive entries and source contributions, and their report types |
| [Materializing and trees](@/docs/api/materializing.md) | `collapse_into`, `dump_to_directory`, `extract_file`, the dry-run plan, trees, `serialize_value` |
| [VfsFile](@/docs/api/files.md) | The file behind a key: loose or archived, its path and name, `open` |
| [Keys and paths](@/docs/api/keys.md) | `NormalizedPath`, `VfsKeyInput`, `normalize_host_path`, the globs, `SourceId`, `NormalizedKey`, `ContentDigest` |
| [LayerIndex](@/docs/api/layer-index.md) | The provider-occurrence index, sources, provider chains and provenance |
| [ConflictIndex and reports](@/docs/api/conflicts.md) | Source-level conflicts, shadowed sources, source diffs, and `diff_directory` |
| [Locks and drift](@/docs/api/lock-drift.md) | `VfsLock`, and what changed since one was taken |
| [Semantic analysis](@/docs/api/semantic.md) | `analyze_pair`, asset classes, and semantic conflict reports |
| [Simulation and planning](@/docs/api/planning.md) | Reorder simulations, impact scoring, candidate directories, and the order solver |
| [Dump, run, collect](@/docs/api/run.md) | `run_setup`, `run_finalize`, snapshots and changed files |
| [Archives](@/docs/api/archives.md) | What opens with which feature, and the `archives` module |
| [experimental](@/docs/api/experimental.md) | Policies, the constraint solver, and the knowledge base: public, not stable |
| [VfsExtension](@/docs/luau/extension.md) | The `lua` module: the l3i extension providing `@dream/vfs` |

## Features

No feature is on by default. Without any, the crate reads loose files only.

| Feature | Adds |
|---|---|
| `beth-archives` | BSA and BA2 archives, through [dream_archive](https://crates.io/crates/dream_archive); see [Archives](@/docs/api/archives.md) |
| `zip` | ZIP, PK3 and JPK archives, through [zip](https://crates.io/crates/zip): stored, deflate, LZMA, and Zstandard through [ruzstd](https://crates.io/crates/ruzstd) |
| `serialize` | `Serialize` on the report types, [`serialize_value`](@/docs/api/materializing.md#serializing-anything) and `VFS::serialize_from_tree`, the re-exports below, and structural JSON and TOML comparison in [semantic analysis](@/docs/api/semantic.md) |
| `lua` | The [`lua` module](@/docs/luau/extension.md), an l3i extension providing `@dream/vfs`. It needs l3i's toolchain: clang, lld and cross-language thin LTO; [Embedding Luau](@/docs/luau-hosts.md) has the setup |
| `luau-analysis` | `lua` and l3i's `analysis`, for the crate's own typed tests. Nothing a program needs |

The `vfstool` program builds it with `serialize` and `beth-archives`. docs.rs builds it with
`beth-archives`, `zip`, `serialize` and `lua`. Without an archive feature, archive lists are
ignored and the archive reports are empty, but every type and method that does not name an
archive handle is still there, so one program can compile either way.

The crate needs Rust 1.88, edition 2024.

## What is stable

The items re-exported at the crate root are the promoted 1.0 API, and the ones to use. Among the
public modules, a few items are not re-exported at the root:

- `archives`: the low-level archive functions and types, on [Archives](@/docs/api/archives.md).
- `directory_node::DirectoryNode`, the node of a `DisplayTree`, and `vfs_file::{FileType,
  ArchiveReference}`.
- `analysis`: provenance records, and the simulation, impact and candidate-plan types on
  [Simulation and planning](@/docs/api/planning.md).
- `run::hash_file`.

`experimental` is public and unstable on purpose: its policies, solver and knowledge base may
change in any release. [experimental](@/docs/api/experimental.md) documents it as it is.

## Keys and host paths

A VFS key such as `textures/tx_wood.dds` and a host path such as `/games/Data Files` are different
things, and the API keeps them apart. Keys are byte strings in [`NormalizedPath`](@/docs/api/keys.md#normalizedpath),
taken as any [`VfsKeyInput`](@/docs/api/keys.md#vfskeyinput) and normalized on the way in; no key
needs to be UTF-8. Host paths are `Path` and `PathBuf`, kept exactly as given, and compared as
`Path` values unless a method says it folds them.

## Serialization re-exports

With `serialize`, the crate re-exports the serialization crates it uses, so a program writes the
same formats with the same versions without depending on them itself:

| Re-export | Crate |
|---|---|
| `vfstool_lib::serde` | [serde](https://crates.io/crates/serde) |
| `vfstool_lib::serde_json` | [serde_json](https://crates.io/crates/serde_json) |
| `vfstool_lib::serde_yaml` | [serde_yaml](https://crates.io/crates/serde_yaml) |
| `vfstool_lib::toml` | [toml](https://crates.io/crates/toml) |

To derive `Serialize` through the re-export, name it: `#[serde(crate = "vfstool_lib::serde")]`,
as the program does for its own reports.

Every `PathBuf` in a report type serializes as a string. serde refuses a path that is not UTF-8;
these write one with U+FFFD in place of each invalid sequence, as `NormalizedKey` does, while the
report in memory keeps the exact bytes.

## Standard output and error

The library writes nothing to the process's streams: they belong to the program. What a call
leaves out comes back in its result, for the caller to print or not.
[`collapse_into`](@/docs/api/materializing.md#writing-files-out) and
[`dump_to_directory`](@/docs/api/materializing.md#writing-files-out) return a
`MaterializationReport` listing each winner they skipped, and
[`diff_directory`](@/docs/api/conflicts.md) lists what its walk could not read in `unreadable`.
