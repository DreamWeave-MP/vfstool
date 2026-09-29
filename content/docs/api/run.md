+++
title = "Dump, run, collect"
description = "run_setup, run_finalize and the snapshots between them: writing the merged VFS to a directory, what hard links mean for your files, and which outputs count as changed."
weight = 120

[extra]
kind = "api"
+++

Some tools want one data directory, not a load order. The run workflow gives them one: write every
resolved file into a scratch directory, record what each file held, let the tool work there, then
copy out whatever it created or changed. `vfstool run` is this workflow with a child process in
the middle; [Running tools on the merged VFS](@/docs/run.md) covers the command. The library leaves
the middle to you.

```rust
use std::{
    fs,
    path::{Path, PathBuf},
};

use vfstool_lib::{VFS, run_finalize_tracked, run_setup_tracked};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-run-example");
    let data = root.join("Data Files");
    let merged = root.join("merged");
    let output = root.join("output");
    fs::create_dir_all(data.join("Textures"))?;
    fs::write(data.join("Textures/Tx_Rock.dds"), b"rock")?;
    fs::write(data.join("readme.txt"), b"hello")?;

    let vfs = VFS::from_directories([&data], None);
    let (count, baseline) = run_setup_tracked(&vfs, &merged, false)?;
    assert_eq!(count, 2);
    assert!(merged.join("textures/tx_rock.dds").is_file()); // keys, not original spellings

    // What a tool might do in the merged tree: add a file, change one, delete one.
    fs::write(merged.join("textures/tx_rock_nm.dds"), b"normal map")?;
    fs::write(merged.join("readme.txt"), b"hello again")?;
    fs::remove_file(merged.join("textures/tx_rock.dds"))?;

    let copied = run_finalize_tracked(&merged, &baseline, &output)?;
    let captured: Vec<&Path> = copied.iter().map(|(relative, _)| relative.as_path()).collect();
    assert_eq!(captured, [PathBuf::from("readme.txt"), Path::new("textures").join("tx_rock_nm.dds")]);
    assert_eq!(fs::read(output.join("readme.txt"))?, b"hello again");
    assert_eq!(fs::read(data.join("readme.txt"))?, b"hello"); // copied, so the source is untouched

    fs::remove_dir_all(&root)
}
```

The deletion is not reported, and `tx_rock.dds` is still in `Data Files`.

## Setting up

{{ api_signature(value="fn run_setup(vfs: &VFS, merged_dir: &Path, use_hardlinks: bool) -> io::Result<(usize, Snapshot)>") }}

1. If `merged_dir` exists, removes it and everything in it, then creates it again, with any
   missing parents. Pass a scratch directory: whatever was there is gone. `vfstool run` refuses a
   directory that is not empty before it gets this far; the library does not.
2. Writes every resolved key as a file under `merged_dir`, at the key's normalized, lowercase path,
   in parallel. A loose winner is hard-linked when `use_hardlinks` is set, and copied when it is
   not or when the link would cross file systems. An archive winner is extracted.
3. Hashes every file in `merged_dir`, as `snapshot_directory` does.

Returns the number of files written and the snapshot.

It fails, possibly with some files already written, when a key cannot be written safely (checked
before anything is written) or cannot be a path on this platform, when a loose winner no longer
exists (`source for VFS key 'KEY' no longer exists at PATH`), when an archive entry cannot be read,
and on any other I/O error.

{% callout(kind="warning", title="Hard links are the same file") %}
With `use_hardlinks`, a file in `merged_dir` and the loose file it came from are one file under two
names. A tool that writes into it in place changes the file in your data directory, and
`run_finalize` then copies the change out as well. Pass `false` for any tool you have not checked.
{% end %}

```rust
use std::fs;

use vfstool_lib::{VFS, run_setup};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-hardlink-example");
    let data = root.join("Data Files");
    let merged = root.join("merged");
    fs::create_dir_all(&data)?;
    fs::write(data.join("readme.txt"), b"original")?;

    let vfs = VFS::from_directories([&data], None);
    run_setup(&vfs, &merged, true)?;
    fs::write(merged.join("readme.txt"), b"edited in place")?;
    assert_eq!(fs::read(data.join("readme.txt"))?, b"edited in place");

    fs::remove_dir_all(&root)
}
```

{{ api_signature(value="fn run_setup_tracked(vfs: &VFS, merged_dir: &Path, use_hardlinks: bool) -> io::Result<(usize, MetadataSnapshot)>") }}

`run_setup`, with a `MetadataSnapshot`: size and modification time beside each hash. This is the
one `vfstool run` uses.

## Collecting

{{ api_signature(value="fn run_finalize(merged_dir: &Path, baseline: &Snapshot, output_dir: &Path) -> io::Result<Vec<(PathBuf, PathBuf)>>") }}

Finds what changed with `changed_files(merged_dir, baseline)`, and copies each of those files to
the same relative path under `output_dir`, creating directories and replacing files already there.
A destination that already is the changed file, as when `output_dir` is a data directory and the
tool rewrote its file in place through a hard link, holds the new content already and is left as
it is. Returns `(relative path, destination)` for each file, sorted by relative path. It does not remove
`merged_dir`. Call it only if the tool succeeded; `vfstool run` does not collect after a failure.

{{ api_signature(value="fn run_finalize_tracked(merged_dir: &Path, baseline: &MetadataSnapshot, output_dir: &Path) -> io::Result<Vec<(PathBuf, PathBuf)>>") }}

`run_finalize` for a `MetadataSnapshot`, through `changed_files_metadata`.

### What counts as changed

{{ api_signature(value="fn changed_files<S: BuildHasher + Sync>(dir: &Path, baseline: &HashMap<PathBuf, [u8; 32], S>) -> io::Result<Vec<PathBuf>>") }}

Walks `dir` and returns, sorted, the relative path of every regular file that is not in `baseline`
or whose BLAKE3 hash differs from the one recorded. Files that were deleted are not reported.
Symbolic links are neither followed nor reported, so a tool that writes its output as a link has it
ignored. A file that cannot be read, or an entry the walk cannot read, is an error. The hasher is
generic, so any `HashMap` with the right key and value types works.

{{ api_signature(value="fn changed_files_metadata<S: BuildHasher + Sync>(dir: &Path, baseline: &HashMap<PathBuf, SnapshotEntry, S>) -> io::Result<Vec<PathBuf>>") }}

The same answer from a `MetadataSnapshot`. A file not in the baseline is reported without being
hashed; every other file is hashed, even when its size and modification time match, because a tool
can rewrite a file in place and keep both.

## Snapshots

{{ api_signature(value="fn snapshot_directory(dir: &Path) -> io::Result<Snapshot>") }}

Hashes every regular file under `dir`, in parallel, keyed by its path relative to `dir`. Symbolic
links are not followed. Any walk or read error is an error.

{{ api_signature(value="fn snapshot_directory_metadata(dir: &Path) -> io::Result<MetadataSnapshot>") }}

The same, with each file's size and modification time.

{{ api_signature(value="fn hash_file(path: &Path) -> io::Result<[u8; 32]>") }}

The BLAKE3 hash of a file's contents, read 64 KiB at a time. Not re-exported at the crate root:
`vfstool_lib::run::hash_file`.

{{ api_signature(value="type Snapshot = HashMap<PathBuf, [u8; 32]>") }}

Relative path to BLAKE3 hash.

{{ api_signature(value="type MetadataSnapshot = HashMap<PathBuf, SnapshotEntry>") }}

Relative path to hash, size and modification time.

{{ api_signature(value="struct SnapshotEntry") }}

`Debug`, `Clone`.

| Field | Meaning |
|---|---|
| `hash: [u8; 32]` | The BLAKE3 hash when the snapshot was taken |
| `size: u64` | The size in bytes |
| `modified: Option<SystemTime>` | The modification time, where the platform reports one |
