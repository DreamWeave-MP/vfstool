+++
title = "Locks and drift"
description = "lock_manifest and diff_against_lock: what a lock records, how its hashes are taken, every kind of drift, and the lock file's shape."
weight = 90

[extra]
kind = "api"
+++

A lock is a VFS's winners written down: for every key, the source that wins it, the winning
content's hash and size, and how many providers the key has. Keep one, and
`diff_against_lock` later says which keys changed and how. Both are `LayerIndex` methods, and both
take the VFS the index belongs to; see [One index, one VFS](@/docs/api/layer-index.md).
`vfstool lock` and `vfstool drift` are these two calls; [Validating and
locking](@/docs/checking.md) uses them from the command line.

```rust
use std::fs;

use vfstool_lib::{DriftKind, VFS, VfsLock, serde_yaml};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join("vfstool-lock-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    fs::create_dir_all(base.join("textures"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("textures/tx_rock.dds"), b"original")?;
    fs::write(patch.join("textures/tx_rock.dds"), b"retextured")?;

    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch], None);
    let lock = layer.lock_manifest(&vfs)?;
    let text = serde_yaml::to_string(&lock)?;
    print!("{text}");

    // Later: the retexture is updated, and a new file appears.
    fs::write(patch.join("textures/tx_rock.dds"), b"retextured again")?;
    fs::write(patch.join("textures/tx_moss.dds"), b"moss")?;

    let saved: VfsLock = serde_yaml::from_str(&text)?;
    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch], None);
    let drift = layer.diff_against_lock(&vfs, &saved)?;

    let rows: Vec<_> = drift.entries.iter().map(|entry| (entry.key.display().to_string(), entry.kind)).collect();
    assert_eq!(rows, [
        ("textures/tx_moss.dds".to_owned(), DriftKind::Added),
        ("textures/tx_rock.dds".to_owned(), DriftKind::WinnerHashChanged),
    ]);
    assert_eq!(drift.counts[&DriftKind::Added], 1);

    fs::remove_dir_all(&root)?;
    Ok(())
}
```

It prints the lock, through the `serde_yaml` the crate re-exports with its `serialize` feature. On
Linux, where the temporary directory is `/tmp`:

```yaml
schema_version: 1
entries:
- key: textures/tx_rock.dds
  winner_source: /tmp/vfstool-lock-example/Rock Retexture
  winner_kind: loose_dir
  winner_hash_blake3: 27f4c63c028dd820329d149dd9217276b8fc35ec62263c077068b60afccad179
  winner_size: 10
  provider_count: 2
```

## lock_manifest

{{ api_signature(value="fn lock_manifest(&self, vfs: &VFS) -> io::Result<VfsLock>") }}

One entry per key of this index whose winner the VFS agrees on. For each, the winning source's
path and kind, the number of providers in the key's chain, and the BLAKE3 hash and size of the
winning content. Loose winners are read from disk; archive winners are read out of their archives.
A loose winner that no longer exists gets no hash and no size. A file or archive entry that cannot
be read makes the whole call an error.

The keys are hashed in parallel, and the entries sorted by key, component by component, the way
`Path` orders. The same files in the same places give the same lock, byte for byte.
`schema_version` is [`VFS_LOCK_SCHEMA_VERSION`](#vfs-lock-schema-version).

## diff_against_lock

{{ api_signature(value="fn diff_against_lock(&self, vfs: &VFS, expected: &VfsLock) -> io::Result<DriftReport>") }}

Builds the current lock, as `lock_manifest` does, and compares it with `expected`, key by key.
Keys and winning sources are compared as the text a lock file holds, with bytes that are not UTF-8
read as U+FFFD, so a lock written to a file and read back matches the VFS it came from. Otherwise
they are compared exactly: a lock edited by hand to spell a key another way reads as that key
removed and the other added. An entry carries the current key, or the lock's for `Removed`.

| Kind | When |
|---|---|
| `Added` | The key is in the current lock and not in `expected` |
| `Removed` | The key is in `expected` and not in the current lock |
| `WinnerSourceChanged` | A different source path wins the key |
| `WinnerHashChanged` | The winning content's hash differs, including a hash that appeared or disappeared |
| `ProviderCountChanged` | The key has a different number of providers |

A key in both locks gets one entry for each of the last three that applies, so a key can appear up
to three times. A change of `winner_kind` or `winner_size` alone is not reported; a size cannot
change without the hash changing too.

Entries are sorted by key, then by kind in the order of the table. `counts` has a total for each
kind that occurs. An empty `entries` means no drift.

Errors: `expected.schema_version` other than 1 is `InvalidData`, with
`unsupported VFS lock schema_version N; expected 1`. Building the current lock can fail as
`lock_manifest` does.

## VfsLock

{{ api_signature(value="struct VfsLock") }}

A lock manifest. `Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`, which is how a
lock is written to a file and read back. `vfstool lock` writes YAML, JSON or TOML; `vfstool drift`
reads any of them.

| Field | Meaning |
|---|---|
| `schema_version: u32` | The format's version: 1 |
| `entries: Vec<VfsLockEntry>` | One per key, sorted |

## VfsLockEntry

{{ api_signature(value="struct VfsLockEntry") }}

`Debug`, `Clone`; `Serialize` and `Deserialize` with `serialize`.

| Field | Meaning |
|---|---|
| `key: PathBuf` | The normalized key |
| `winner_source: PathBuf` | The winning source's path |
| `winner_kind: SourceKind` | `loose_dir` or `archive` when serialized |
| `winner_hash_blake3: Option<String>` | The BLAKE3 hash of the winning content, 64 lowercase hex digits |
| `winner_size: Option<u64>` | Its size in bytes |
| `provider_count: usize` | The number of providers in the key's chain, counting a source twice when it provides the key twice |

## VFS_LOCK_SCHEMA_VERSION

{{ api_signature(value="const VFS_LOCK_SCHEMA_VERSION: u32 = 1") }}

The lock format this version writes and the only one `diff_against_lock` accepts.

## DriftReport

{{ api_signature(value="struct DriftReport") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `entries: Vec<DriftEntry>` | Every change, sorted by key, then kind |
| `counts: BTreeMap<DriftKind, usize>` | How many entries of each kind; kinds with none are absent |

## DriftEntry

{{ api_signature(value="struct DriftEntry") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `key: PathBuf` | The key |
| `kind: DriftKind` | What changed |

## DriftKind

{{ api_signature(value="enum DriftKind") }}

`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Ord`, `PartialOrd`, in the order of the
[table above](#diff-against-lock); `Serialize` with `serialize`, as `"added"`, `"removed"`,
`"winner_source_changed"`, `"winner_hash_changed"` and `"provider_count_changed"`.
