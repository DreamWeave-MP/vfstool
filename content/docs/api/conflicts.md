+++
title = "ConflictIndex and reports"
description = "ConflictIndex and its constructors, which sources override which, the conflicts, shadowed and diff reports, and what a new directory would change in a VFS."
weight = 80

[extra]
kind = "api"
+++

A `ConflictIndex` is the load order seen the way a mod manager draws it: for each source, the files
it overrides in earlier sources and the files later sources override in it. It is derived from a
[`LayerIndex`](@/docs/api/layer-index.md) and deliberately narrower:

- It compares sources, not occurrences. A source that provides a key twice counts once.
- Two sources with the same kind and the same path, after `\` becomes `/` and ASCII letters become
  lowercase, are one source: a key the second provides again counts only for the first.
- It keeps only the keys that two or more sources share.

When you need the chain behind a key, or anything about file contents, use the `LayerIndex`.
`vfstool conflicts`, `shadowed` and `diff` print the reports on this page; see
[Conflicts and load order](@/docs/conflicts.md).

```rust
use std::{
    fs,
    path::{Path, PathBuf},
};

use vfstool_lib::ConflictIndex;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-conflicts-example");
    let base = root.join("Data Files");
    let patch = root.join("Rock Retexture");
    let hd = root.join("HD Rocks");
    for (dir, files) in [
        (&base, &["textures/tx_rock.dds", "meshes/rock.nif"][..]),
        (&patch, &["textures/tx_rock.dds"][..]),
        (&hd, &["textures/tx_rock.dds", "textures/tx_rock_nm.dds"][..]),
    ] {
        for file in files {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, file.as_bytes())?;
        }
    }

    let index = ConflictIndex::from_directories([&base, &patch, &hd]);
    let rock = Path::new("textures/tx_rock.dds");
    assert!(index.conflicts[0].is_overridden() && !index.conflicts[0].has_overrides());
    assert!(index.conflicts[1].has_overrides() && index.conflicts[1].is_overridden());
    assert_eq!(index.sources_containing(rock), &[0, 1, 2]);
    assert_eq!(index.displaced_by(1, rock), Some(0));
    assert_eq!(index.overridden_by_dir(1, rock), Some(2));

    let report = index.conflicts_report(true);
    assert_eq!(report.sources[2].overrides, [PathBuf::from("textures/tx_rock.dds")]);

    // Every file in the retexture is covered by HD Rocks.
    let shadowed = index.shadowed_report(true);
    assert_eq!(shadowed.sources.len(), 1);
    assert_eq!(shadowed.sources[0].path, patch);

    let diff = index.diff_report(&base, &hd);
    assert_eq!(diff.higher_priority, hd);
    assert_eq!(diff.only_in_a, ["meshes/rock.nif"]);

    fs::remove_dir_all(&root)
}
```

## ConflictIndex

{{ api_signature(value="struct ConflictIndex") }}

No derived traits.

| Field | Meaning |
|---|---|
| `sources: Vec<PathBuf>` | Every source's path, lowest priority first |
| `conflicts: Vec<SourceConflicts>` | One per source: `conflicts[i]` belongs to `sources[i]` |

The source kinds, the number of keys each source provides, and the map of shared keys are private.

### Building

`VFS::from_directories_with_conflict_index` returns one built from the VFS it builds; see
[VFS](@/docs/api/vfs.md). The constructors below build one without a VFS.

{{ api_signature(value="fn from_directories(dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>) -> Self") }}

Walks each directory, following symbolic links, lowest priority first. Every regular file whose
path relative to the directory is a safe key is recorded; see
[`from_file_lists`](@/docs/api/layer-index.md) for what is not. Entries that cannot be read are
skipped without a message. Every source is a `LooseDir`.

{{ api_signature(value="fn from_directories_with_archives(dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>, archive_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> Self") }}

With the `beth-archives` or `zip` feature. The archives come first, below every directory, in the
order given; then the directories, as in `from_directories`. `archive_paths` are paths to the
archive files themselves, not the names `openmw.cfg` lists. An archive that cannot be opened stays
in `sources` and provides nothing.

{{ api_signature(value="fn from_file_lists(sources: impl IntoIterator<Item = (PathBuf, Vec<PathBuf>)>) -> Self") }}

From `(source path, files)` pairs, lowest priority first, without reading the disk. Every source
is a `LooseDir`. The files are turned into keys as in
[`LayerIndex::from_file_lists`](@/docs/api/layer-index.md), which this calls.

{{ api_signature(value="fn from_layer_index(layer: &LayerIndex) -> Self") }}

Derives the conflict view from a provider index. `sources` and the source kinds are copied from
`layer.sources`.

### Asking about one key

{{ api_signature(value="fn sources_containing(&self, path: &Path) -> &[usize]") }}

The distinct sources that provide `path`, lowest priority first. Empty when fewer than two sources
provide it: this index keeps only shared keys. `path` may be spelled any way.

{{ api_signature(value="fn displaced_by(&self, source_index: usize, path: &Path) -> Option<usize>") }}

The source whose copy of `path` source `source_index`'s copy directly replaces: the next lower
source that has it. `None` when `source_index` is the lowest of them, does not have `path`, or
`path` is not shared.

{{ api_signature(value="fn overridden_by_dir(&self, source_index: usize, path: &Path) -> Option<usize>") }}

The source whose copy of `path` directly replaces source `source_index`'s: the next higher source
that has it. `None` when `source_index` is the highest of them, does not have `path`, or `path` is
not shared.

### Reports

{{ api_signature(value="fn conflicts_report(&self, use_relative: bool) -> ConflictsReport") }}

One entry per source, lowest priority first, including sources with nothing to report. Each lists
the keys it overrides and the keys overridden in it, sorted.

With `use_relative`, the lists hold keys. Without it, a loose source's entries are its path joined
with the key, and an archive's are `ARCHIVE::KEY`. The key is the normalized, lowercase spelling,
so on a case-sensitive file system a joined path names the real file only when the file is spelled
that way on disk.

{{ api_signature(value="fn shadowed_report(&self, use_relative: bool) -> ShadowedReport") }}

`shadowed_report_with_files(use_relative, true)`.

{{ api_signature(value="fn shadowed_report_with_files(&self, use_relative: bool, list_files: bool) -> ShadowedReport") }}

The sources every one of whose keys a later source provides again: sources that contribute nothing
to the result. A source that provides no keys at all is not listed. With `list_files`, each entry
lists the covered keys, sorted and spelled as in `conflicts_report`; without it, the list is empty,
and `serialize` leaves it out.

{{ api_signature(value="fn diff_report(&self, source_a: &Path, source_b: &Path) -> DiffReport") }}

Compares two sources' keys. Each path names the sources in `sources` spelled exactly like it, or,
when there are none, those that equal it with ASCII case and `\` folded, as a file system that
ignores case would find them. Their keys come from the index, so archives and indexes built from
file lists compare by what they hold. A path that names no source is read from disk as a data
directory, loose files only, so it need not be in the index. `source_a` and `source_b` in the
report are the index's spelling, or the path as given for one it does not have.
`higher_priority` is whichever of the two comes later in `sources`; when either is not in
`sources`, it is `source_b`.

## SourceConflicts

{{ api_signature(value="struct SourceConflicts") }}

One source's side of the conflicts. `Debug`, `Default`. Both sets hold keys, in no particular
order.

| Field | Meaning |
|---|---|
| `overrides: AHashSet<PathBuf>` | Keys this source provides that an earlier source also provides |
| `overridden_by: AHashSet<PathBuf>` | Keys this source provides that a later source also provides |

{{ api_signature(value="fn has_overrides(&self) -> bool") }}

`overrides` is not empty: a mod manager's green arrow.

{{ api_signature(value="fn is_overridden(&self) -> bool") }}

`overridden_by` is not empty: the red one. A source in the middle of the order can have both.

## ConflictsReport

{{ api_signature(value="struct ConflictsReport") }}

`Serialize` with `serialize`; no other traits.

| Field | Meaning |
|---|---|
| `sources: Vec<ConflictSourceEntry>` | One per source, lowest priority first |

## ConflictSourceEntry

{{ api_signature(value="struct ConflictSourceEntry") }}

`Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `path: PathBuf` | The source's path |
| `overrides: Vec<PathBuf>` | What it overrides, sorted |
| `overridden_by: Vec<PathBuf>` | What later sources override in it, sorted |

## ShadowedReport

{{ api_signature(value="struct ShadowedReport") }}

`Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `sources: Vec<ShadowedSource>` | The fully covered sources, lowest priority first |

## ShadowedSource

{{ api_signature(value="struct ShadowedSource") }}

`Serialize` with `serialize`; an empty `shadowed_files` is not serialized.

| Field | Meaning |
|---|---|
| `path: PathBuf` | The source's path |
| `shadowed_files: Vec<PathBuf>` | Every key it provides, sorted; empty without `list_files` |

## DiffReport

{{ api_signature(value="struct DiffReport") }}

`Serialize` with `serialize`.

| Field | Meaning |
|---|---|
| `source_a: PathBuf` | The first directory, as passed |
| `source_b: PathBuf` | The second directory, as passed |
| `higher_priority: PathBuf` | Which of the two wins where they overlap |
| `shared: Vec<String>` | Keys in both, sorted |
| `only_in_a: Vec<String>` | Keys only in `source_a`, sorted |
| `only_in_b: Vec<String>` | Keys only in `source_b`, sorted |

Keys are strings here; bytes that are not UTF-8 become U+FFFD.

## What a new directory would change

{{ api_signature(value="fn diff_directory<P: AsRef<Path> + Sync>(&self, dir: P) -> DirectoryDiff<'_>") }}

A method of `VFS`. Walks `dir`, following symbolic links, and sorts each regular file whose
relative path is a safe key into one of two lists: a conflict when the key already resolves in the
VFS, an addition when it does not. Two files in `dir` with the same key are both listed. Nothing is
read but the directory listing, and the VFS is not changed.

The walk runs in parallel, so the lists are in no particular order. An entry that cannot be read is
skipped, with `vfstool: warning: failed to walk 'DIR': ERROR` on standard error.
[`LayerIndex::plan_candidate_directory`](@/docs/api/planning.md) builds a sorted plan from it.

```rust
use std::{fs, path::Path};

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-diff-directory-example");
    let data = root.join("Data Files");
    let candidate = root.join("New Mod");
    fs::create_dir_all(data.join("textures"))?;
    fs::create_dir_all(candidate.join("Textures"))?;
    fs::write(data.join("textures/tx_rock.dds"), b"old")?;
    fs::write(candidate.join("Textures/Tx_Rock.dds"), b"new")?;
    fs::write(candidate.join("Textures/Tx_Moss.dds"), b"new")?;

    let vfs = VFS::from_directories([&data], None);
    let diff = vfs.diff_directory(&candidate);

    let (key, incoming, current) = &diff.conflicts[0];
    assert_eq!(key, Path::new("textures/tx_rock.dds"));
    assert_eq!(incoming.path(), candidate.join("Textures/Tx_Rock.dds"));
    assert_eq!(current.path(), data.join("textures/tx_rock.dds"));
    assert_eq!(diff.additions.len(), 1);
    assert_eq!(diff.additions[0].0, Path::new("textures/tx_moss.dds"));

    fs::remove_dir_all(&root)
}
```

{{ api_signature(value="struct DirectoryDiff<'vfs>") }}

What `diff_directory` returns, borrowing the VFS. No derived traits.

| Field | Meaning |
|---|---|
| `conflicts: Vec<(PathBuf, VfsFile, &'vfs VfsFile)>` | The key, the file in the directory, and the VFS's current winner for it |
| `additions: Vec<(PathBuf, VfsFile)>` | The key and the file in the directory |
