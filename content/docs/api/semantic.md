+++
title = "Semantic analysis"
description = "analyze_pair and how it classifies and compares two versions of a file, and semantic_conflicts: every shared key with hashes, relations to the winner and content deltas."
weight = 100

[extra]
kind = "api"
+++

Two mods ship `settings.ini`. Is the one that wins different in a way that matters, or did someone
reorder the lines? `analyze_pair` answers that for one pair of files, for the formats it
understands, and `LayerIndex::semantic_conflicts` answers it for every key a load order provides
more than once. It classifies; it does not merge, and it knows nothing about NIF, DDS or plugins
beyond whether their bytes are equal.

## analyze_pair

{{ api_signature(value="fn analyze_pair(path: &Path, left: &[u8], right: &[u8]) -> (AssetClass, SemanticDelta)") }}

Classifies the file by `path` and, when the extension does not decide it, by the two contents; then
compares `left` with `right` by that class's rules. `path` is only looked at for its extension;
nothing is read from disk.

```rust
use std::path::Path;

use vfstool_lib::{AssetClass, SemanticDelta, analyze_pair};

fn main() {
    let (class, delta) = analyze_pair(
        Path::new("settings.ini"),
        b"[Weather]\nSunrise=6\nSunset=18\n",
        b"; reordered\n[weather]\n  sunset = 18\nsunrise=6\n",
    );
    assert_eq!(class, AssetClass::Ini);
    assert_eq!(delta, SemanticDelta::CosmeticOnly);

    let (class, delta) = analyze_pair(Path::new("scripts/init.lua"), b"return 1\n", b"return 2\n");
    assert_eq!(class, AssetClass::LuaScript);
    assert!(matches!(delta, SemanticDelta::BehaviorChanging { .. }));

    // Needs the serialize feature; without it, JSON and TOML deltas are Unknown.
    let (_, delta) = analyze_pair(Path::new("data.json"), br#"{"a":1,"b":2}"#, b"{\n  \"b\": 2,\n  \"a\": 1\n}");
    assert_eq!(delta, SemanticDelta::CosmeticOnly);

    let (class, delta) = analyze_pair(Path::new("textures/tx_rock.dds"), b"DDS \0\x01", b"DDS \0\x02");
    assert_eq!(class, AssetClass::Binary);
    assert!(matches!(delta, SemanticDelta::BehaviorChanging { .. }));
}
```

### Classes

The extension decides, ignoring ASCII case:

| Extension | Class |
|---|---|
| `.ini`, `.cfg` | `Ini` |
| `.toml` | `Toml` |
| `.json` | `Json` |
| `.lua` | `LuaScript` |
| `.mws`, `.mwscript` | `MwScriptLike` |
| `.txt`, `.md` | `Text` |
| Anything else, or none | `Binary` if either content contains a zero byte; otherwise `Text` if both are UTF-8; otherwise `Unknown` |

So UTF-8 `.omwscripts` and `.yaml` files are `Text`, and a DDS texture, whose header holds zero
bytes, is `Binary`.

### Comparisons

| Class | `NoOpEquivalent` | `CosmeticOnly` | `BehaviorChanging` summary | `Unknown` |
|---|---|---|---|---|
| `Ini` | Same text | Same sections, keys and values once parsed | `INI keys/values differ` | Either is not UTF-8 |
| `Toml` | Same bytes | Same values once parsed | `TOML semantic values differ` | Either does not parse, or no `serialize` feature |
| `Json` | Same bytes | Same values once parsed | `JSON semantic values differ` | Either does not parse, or no `serialize` feature |
| `LuaScript`, `MwScriptLike`, `Text` | Same text | Same lines once each is trimmed and blank lines dropped | `text content differs after normalization` | Either is not UTF-8 |
| `Binary` | Same bytes | Never | `binary payload differs` | Never |
| `Unknown` | Never | Never | Never | Always |

How an INI file is parsed: every line is trimmed; blank lines and lines starting with `;` or `#`
are skipped; `[section]` starts a section, whose name is trimmed and lowercased, and keys before
the first one are in a section named `global`; `key=value` splits at the first `=`, with the key
trimmed and lowercased and the value trimmed. A key given twice keeps its last value. Any other line
is ignored, so changing it is cosmetic. Values keep their case.

JSON and TOML compare as parsed values: key order, whitespace, and in TOML comments, do not count.
For text classes, indentation and blank lines do not count; comments do.

## semantic_conflicts

{{ api_signature(value="fn semantic_conflicts(&self, vfs: &VFS) -> io::Result<SemanticConflictReport>") }}

`semantic_conflicts_with_opts(vfs, SemanticOpts::default())`: hashes, the winning archive entries
only, and no content comparison.

{{ api_signature(value="fn semantic_conflicts_with_opts(&self, vfs: &VFS, opts: SemanticOpts) -> io::Result<SemanticConflictReport>") }}

A `LayerIndex` method. One entry for every key the index has more than one occurrence of,
including a key one source provides twice, sorted by key. A key whose winner the index and the VFS
disagree on is left out; see [One index, one VFS](@/docs/api/layer-index.md).

For each provider in the key's chain, lowest first, the content's hash and size when it can be had,
and its relation to the winner's hash. Loose files are always read, from disk; one that no longer
exists has no hash. Archive entries are read according to `opts.archive_hash_mode`.

With `opts.include_semantic_deltas`, each provider's content and the winner's are read in full,
under the same rule, and compared with `analyze_pair(key, provider, winner)`. The winner's own
delta is `NoOpEquivalent`. The entry's `asset_class` is the class that comparison found, and stays
`Unknown` without `include_semantic_deltas`.

A file or archive entry that is read and fails, or fails to decompress, makes the whole call an
error. Unknown in the report means not read, by the options or because the content is gone, never a
swallowed error.

```rust
use std::fs;

use vfstool_lib::{AssetClass, SemanticDelta, SemanticOpts, SemanticRelation, VFS};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-semantic-example");
    let base = root.join("Data Files");
    let patch = root.join("Settings Patch");
    let copy = root.join("Settings Copy");
    for dir in [&base, &patch, &copy] {
        fs::create_dir_all(dir)?;
    }
    fs::write(base.join("settings.ini"), "[General]\nFov=75\n")?;
    fs::write(patch.join("settings.ini"), "; wider\n[General]\nFov=90\n")?;
    fs::write(copy.join("settings.ini"), "[general]\nfov = 90\n")?;

    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch, &copy], None);
    let opts = SemanticOpts { include_semantic_deltas: true, ..SemanticOpts::default() };
    let report = layer.semantic_conflicts_with_opts(&vfs, opts)?;

    let conflict = &report.entries[0];
    assert_eq!(conflict.winner.path, copy);
    assert_eq!(conflict.asset_class, AssetClass::Ini);
    assert_eq!(conflict.distinct_versions, 3);
    assert!(!conflict.all_identical);

    let base_side = &conflict.providers[0];
    assert_eq!(base_side.relation, SemanticRelation::DifferentFromWinner);
    assert!(matches!(base_side.semantic_delta_to_winner, Some(SemanticDelta::BehaviorChanging { .. })));
    let patch_side = &conflict.providers[1];
    assert_eq!(patch_side.semantic_delta_to_winner, Some(SemanticDelta::CosmeticOnly));

    fs::remove_dir_all(&root)
}
```

## SemanticOpts

{{ api_signature(value="struct SemanticOpts") }}

`Debug`, `Clone`, `Copy`, `Default`.

| Field | Default | Meaning |
|---|---|---|
| `archive_hash_mode: ArchiveHashMode` | `WinnerOnly` | Which archive providers to read |
| `include_semantic_deltas: bool` | `false` | Read contents in full and compare each provider with the winner |

## ArchiveHashMode

{{ api_signature(value="enum ArchiveHashMode") }}

Opening and decompressing every overridden archive entry in a large load order is expensive, so it
is a choice. Archive entries are read from the exact entry the VFS holds; nothing is looked up again
by name. `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`; `Serialize` with `serialize`, as
`"disabled"`, `"winner_only"` and `"all_providers"`.

| Variant | Reads |
|---|---|
| `Disabled` | No archive entries. Their hashes and relations are unknown |
| `WinnerOnly` | Archive entries that win their key |
| `AllProviders` | Every archive entry in the chain |

## SemanticConflictReport

{{ api_signature(value="struct SemanticConflictReport") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `entries: Vec<SemanticConflict>` | One per shared key, sorted by key |

## SemanticConflict

{{ api_signature(value="struct SemanticConflict") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `key: PathBuf` | The key |
| `winner: SourceMeta` | The winning source |
| `providers: Vec<SemanticProvider>` | Every occurrence, lowest priority first; the last is the winner |
| `asset_class: AssetClass` | The class `analyze_pair` found; `Unknown` without `include_semantic_deltas` |
| `all_identical: bool` | Every provider, the winner included, was hashed, and all hashes are equal |
| `distinct_versions: usize` | How many different hashes the hashed providers have |

## SemanticProvider

{{ api_signature(value="struct SemanticProvider") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `source: SourceMeta` | The source |
| `relation: SemanticRelation` | This content against the winner's |
| `hash_blake3: Option<String>` | The BLAKE3 hash, 64 lowercase hex digits, when the content was read |
| `size: Option<u64>` | Its size in bytes, under the same condition |
| `semantic_delta_to_winner: Option<SemanticDelta>` | With `include_semantic_deltas`, when both contents were read: how this version differs from the winner's |

## SemanticRelation

{{ api_signature(value="enum SemanticRelation") }}

`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`; `Serialize` and `Deserialize` with `serialize`, as
`"identical_to_winner"`, `"different_from_winner"` and `"unknown"`.

| Variant | Meaning |
|---|---|
| `IdenticalToWinner` | Same hash as the winner. The winner itself is this |
| `DifferentFromWinner` | A different hash |
| `Unknown` | This provider or the winner was not hashed |

## AssetClass

{{ api_signature(value="enum AssetClass") }}

The file families [above](#classes). `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`; `Serialize` and
`Deserialize` with `serialize`, in snake case: `"ini"`, `"toml"`, `"json"`, `"lua_script"`,
`"mw_script_like"`, `"text"`, `"binary"`, `"unknown"`.

## SemanticDelta

{{ api_signature(value="enum SemanticDelta") }}

How one version differs from another. `Debug`, `Clone`, `PartialEq`, `Eq`; `Serialize` and
`Deserialize` with `serialize`, in snake case, the struct variant as
`{ behavior_changing: { change_summary: [...] } }`.

| Variant | Meaning |
|---|---|
| `NoOpEquivalent` | No difference |
| `CosmeticOnly` | Different bytes, same meaning by the class's rules |
| `BehaviorChanging { change_summary: Vec<String> }` | A difference that counts. `change_summary` holds one sentence, from the [table](#comparisons) |
| `Unknown` | Not comparable |
