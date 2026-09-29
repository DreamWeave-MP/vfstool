+++
title = "LayerIndex"
description = "The provider-occurrence index: every key's providers from low to high priority, the spelling each used, per-source counts, and provenance with hashes."
weight = 70

[extra]
kind = "api"
+++

A `LayerIndex` records, for every normalized key, which sources provide it, lowest priority first,
and the path each source spelled it with. It is a [`VFS`](@/docs/api/vfs.md)'s provider stacks as
data, and the base of every report that explains a load order: provenance on this page,
[locks and drift](@/docs/api/lock-drift.md), [semantic conflicts](@/docs/api/semantic.md),
[simulations and the solver](@/docs/api/planning.md), and the
[`ConflictIndex`](@/docs/api/conflicts.md) derived from it.

There are three ways to get one:

- `vfs.layer_index()`, built from the VFS the first time it is asked for, then kept.
- `VFS::from_directories_with_layer_index`, which builds the VFS and its index from one walk.
- `LayerIndex::from_file_lists`, from lists of files, without reading the disk.

Every entry is an occurrence, not a source. A source that holds both `Textures/Tx_Rock.dds` and
`textures/tx_rock.dds`, which a case-sensitive file system allows, provides `textures/tx_rock.dds`
twice. [How files resolve](@/docs/resolution.md) explains the order itself.

## One index, one VFS

The methods that take a `vfs` read file contents through it and take its word for the winner. For
each key they check that this index's highest provider is the one the VFS resolves to, by its
position in the key's provider stack. Where the two disagree, the key is left out: `provenance`
returns `None`, and `lock_manifest` and `semantic_conflicts` write no entry for it. Pass the index
that belongs to the VFS: `vfs.layer_index()`, or the one `from_directories_with_layer_index`
returned.

An index built from a VFS lists only the sources that provide at least one key. A data directory
with no files in it is in neither `sources` nor any chain, so a source index here can be smaller
than the source's position in the list of directories the VFS was built from.

## LayerIndex

{{ api_signature(value="struct LayerIndex") }}

`Debug`, `Clone`.

| Field | Meaning |
|---|---|
| `sources: Vec<SourceMeta>` | Every source, lowest priority first. A source index anywhere on this page is a position in this list |

The key map and the recorded spellings are private; the methods below read them.

### from_file_lists

{{ api_signature(value="fn from_file_lists(sources: impl IntoIterator<Item = (SourceMeta, Vec<PathBuf>)>) -> Self") }}

One `(source, files)` pair per source, lowest priority first. `files` are paths relative to the
source, in any spelling. Nothing is read from disk, so the files need not exist.

Each path becomes a key: `\` becomes `/`, ASCII letters become lowercase, and `.` components and
repeated separators are dropped. A path that is absolute, contains a `..` component, starts with a
drive letter such as `C:`, or is empty is skipped. The spelling passed in is kept as that
occurrence's original path. Every source is kept in `sources`, even one whose paths were all
skipped.

```rust
use std::path::{Path, PathBuf};

use vfstool_lib::{LayerIndex, SourceKind, SourceMeta};

fn main() {
    let loose = |path: &str| SourceMeta { path: PathBuf::from(path), kind: SourceKind::LooseDir };
    let layer = LayerIndex::from_file_lists([
        (loose("/games/Data Files"), vec![PathBuf::from("Textures/Tx_Rock.dds"), PathBuf::from("Meshes/Rock.nif")]),
        (loose("/mods/Rock Retexture"), vec![PathBuf::from(r"textures\tx_rock.dds")]),
        (loose("/mods/Broken"), vec![PathBuf::from("../escape.dds"), PathBuf::from("/etc/passwd")]),
    ]);

    assert_eq!(layer.sources.len(), 3);
    assert_eq!(layer.keys().len(), 2);
    assert_eq!(layer.sources_containing("TEXTURES/TX_ROCK.DDS"), &[0, 1]);

    let chain = layer.provider_chain(Path::new("textures/tx_rock.dds"));
    assert_eq!(chain[0].original_path, Path::new("Textures/Tx_Rock.dds"));
    assert_eq!(chain[1].source.path, Path::new("/mods/Rock Retexture"));
    assert_eq!(layer.duplicate_keys().len(), 1);
}
```

### keys

{{ api_signature(value="fn keys(&self) -> Vec<NormalizedPath>") }}

Every key, sorted by its bytes. A new vector on each call.

### sources_containing

{{ api_signature(value="fn sources_containing<K: VfsKeyInput + ?Sized>(&self, path: &K) -> &[usize]") }}

The source indices that provide `path`, lowest priority first, one per occurrence: a source that
provides the key twice appears twice. Empty when nothing provides it. `path` may be spelled any way
a [`VfsKeyInput`](@/docs/api/keys.md) accepts; a spelling that is already normalized is looked up
without allocating.

### provider_chain

{{ api_signature(value="fn provider_chain(&self, path: &Path) -> Vec<LayerProvider>") }}

The occurrences that provide `path`, lowest priority first, each as a
[`LayerProvider`](#layerprovider): the source, its index, the occurrence's position in the chain,
the key, and the spelling recorded for it. The last one is the winner. Empty when nothing provides
`path`, which may be spelled any way.

### provider_original_path

{{ api_signature(value="fn provider_original_path<K: VfsKeyInput + ?Sized>(&self, source_index: usize, path: &K) -> Option<&Path>") }}

The spelling source `source_index` gave for `path`: what was passed to `from_file_lists`, or, in
an index built from a VFS, the file's path relative to its data directory. For archive entries in a
VFS's index, and for a loose file that does not lie under its source's path, it is the key itself.
When the source provides the key more than once, the first spelling. `None` when that source does
not provide `path`.

### duplicate_keys

{{ api_signature(value="fn duplicate_keys(&self) -> Vec<NormalizedPath>") }}

The keys with more than one occurrence, sorted by their bytes. A key one source provides twice is
one of them.

### source_id_for_path

{{ api_signature(value="fn source_id_for_path(&self, path: &Path) -> Option<SourceId>") }}

The first source whose path equals `path`, as a [`SourceId`](@/docs/api/keys.md). The comparison
is `Path` equality: no case folding and no separator conversion. `None` when no source has that
path.

### source_by_id

{{ api_signature(value="fn source_by_id(&self, source_id: SourceId) -> Option<&SourceMeta>") }}

`sources[source_id.as_index()]`, or `None` past the end.

### source_contributions

{{ api_signature(value="fn source_contributions(&self) -> SourceContributionReport") }}

One row per source, in `sources` order, including sources that provide nothing. Everything is
counted per occurrence: how many of the source's occurrences win, override a different source,
are overridden by a different source, are the only provider of their key, share their key, and are
loose files or archive entries. An occurrence that is only covered by another occurrence from the
same source counts as neither overriding nor overridden. `VFS::source_contributions` returns the
same report for a VFS; [Provider reports](@/docs/api/providers.md) has the fields.

### provenance

{{ api_signature(value="fn provenance(&self, vfs: &VFS, path: &Path, with_hashes: bool) -> io::Result<Option<ProvenanceChain>>") }}

The whole chain for one key, with where each provider's bytes are and, with `with_hashes`, what
they are. `path` may be spelled any way. Returns `Ok(None)` when nothing provides the key, or when
this index's winner for it is not the VFS's (see [One index, one VFS](#one-index-one-vfs)).

With `with_hashes`, every provider is read in full: loose files from disk, archive entries from
their archives, whether they win or not. A loose file that no longer exists gets no hash and no
size. A file or archive entry that cannot be read is an error. Without `with_hashes`, nothing is
read and `hash_blake3` and `size` are `None` throughout.

```rust
use std::{fs, path::Path};

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-provenance-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    fs::create_dir_all(base.join("Textures"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("Textures/Tx_Rock.dds"), b"original")?;
    fs::write(patch.join("textures/tx_rock.dds"), b"retextured")?;

    let (vfs, layer) = VFS::from_directories_with_layer_index([&base, &patch], None);
    let chain = layer
        .provenance(&vfs, Path::new(r"Textures\Tx_Rock.dds"), true)?
        .expect("two sources provide it");

    assert_eq!(chain.key, Path::new("textures/tx_rock.dds"));
    assert_eq!(chain.winner.path, patch);
    assert_eq!(chain.providers[0].resolved_path, base.join("Textures/Tx_Rock.dds").display().to_string());
    assert_eq!(chain.providers[1].size, Some(10));
    assert_eq!(chain.providers[1].hash_blake3.as_ref().map(String::len), Some(64));

    fs::remove_dir_all(&root)
}
```

## ProvenanceChain

{{ api_signature(value="struct ProvenanceChain") }}

What `provenance` returns. Not re-exported at the crate root:
`vfstool_lib::analysis::ProvenanceChain`. `Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `key: PathBuf` | The normalized key |
| `providers: Vec<ProviderRecord>` | Every occurrence, lowest priority first; the last one wins |
| `winner: SourceMeta` | The winning source |

## ProviderRecord

{{ api_signature(value="struct ProviderRecord") }}

One provider in a `ProvenanceChain`: `vfstool_lib::analysis::ProviderRecord`. `Debug`, `Clone`;
`Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `source: SourceMeta` | The source |
| `resolved_path: String` | A loose file's path, as the VFS holds it. An archive entry as `ARCHIVE::ENTRY`, such as `/games/Data Files/Morrowind.bsa::textures/tx_rock.dds` |
| `hash_blake3: Option<String>` | The BLAKE3 hash of the content, 64 lowercase hex digits. Only with `with_hashes`, and only when the content could be found |
| `size: Option<u64>` | The content's size in bytes, under the same conditions |

## LayerProvider

{{ api_signature(value="struct LayerProvider") }}

One occurrence in a `provider_chain`. `Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `source_index: usize` | The source's position in `sources` |
| `provider_index: usize` | The occurrence's position in the key's chain, 0 for the lowest. In an index built from a VFS, it is also the position in the VFS's provider stack for the key |
| `source: SourceMeta` | A copy of the source |
| `key: PathBuf` | The normalized key |
| `original_path: PathBuf` | The spelling recorded for this occurrence; the key when none was |

## SourceMeta

{{ api_signature(value="struct SourceMeta") }}

A source in the load order. `Debug`, `Clone`, `PartialEq`, `Eq`; `Serialize` and `Deserialize`
with `serialize`.

| Field | Meaning |
|---|---|
| `path: PathBuf` | The data directory, or the archive file. As it was given: nothing makes it absolute or canonical. For an archive the VFS found, the path of the archive file in the data directory that holds it |
| `kind: SourceKind` | Which of the two |

## SourceKind

{{ api_signature(value="enum SourceKind") }}

`Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`; `Serialize` and `Deserialize` with
`serialize`, as `"loose_dir"` and `"archive"`. Marked `#[non_exhaustive]`: match it with a
wildcard arm.

| Variant | Meaning |
|---|---|
| `LooseDir` | A data directory of loose files |
| `Archive` | A BSA, BA2, ZIP or PK3 archive |

## Locks, semantic conflicts and planning

The rest of `LayerIndex` has pages of its own:

| Method | Returns | Page |
|---|---|---|
| `lock_manifest(&self, vfs: &VFS)` | The winners, written down | [Locks and drift](@/docs/api/lock-drift.md) |
| `diff_against_lock(&self, vfs: &VFS, expected: &VfsLock)` | What changed since a lock | [Locks and drift](@/docs/api/lock-drift.md) |
| `semantic_conflicts(&self, vfs: &VFS)` | Every shared key, with hashes | [Semantic analysis](@/docs/api/semantic.md) |
| `semantic_conflicts_with_opts(&self, vfs: &VFS, opts: SemanticOpts)` | The same, with archive hashing and content comparison chosen | [Semantic analysis](@/docs/api/semantic.md) |
| `simulate(&self, vfs: &VFS, op: ReorderOp)` | Which winners a reorder would change | [Simulation and planning](@/docs/api/planning.md) |
| `simulate_with_opts(&self, vfs: &VFS, op: ReorderOp, opts: &SimOpts)` | The same, with buckets and a sample size | [Simulation and planning](@/docs/api/planning.md) |
| `simulate_impact(&self, vfs: &VFS, op: ReorderOp, opts: &SimOpts, profile: &ImpactProfile)` | The same changes, scored | [Simulation and planning](@/docs/api/planning.md) |
| `plan_candidate_directory(&self, vfs: &VFS, candidate_dir: &Path, opts: CandidatePlanOpts)` | What adding a directory on top would change | [Simulation and planning](@/docs/api/planning.md) |
| `solve_order(&self, request: &SolveRequest)` | A load order that meets constraints | [Simulation and planning](@/docs/api/planning.md) |
