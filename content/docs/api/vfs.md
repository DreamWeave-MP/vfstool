+++
title = "VFS"
description = "The VFS struct: building one from data directories and archives, looking files up by key, iterating, searching by prefix, substring or regex, and what is left of a data directory."
weight = 10

[extra]
kind = "api"
+++

A `VFS` is what OpenMW builds from `data=` and `fallback-archive=`: every file the load order
provides, under a normalized key, and the file that wins each key. It also keeps the files that
lose. Each key has a provider stack, lowest priority first, and the resolved winner is always the
top of it. [How files resolve](@/docs/resolution.md) describes the rules; this page is the API.

```rust
use std::fs;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-vfs");
    let base = root.join("Data Files");
    let patch = root.join("Patch");
    fs::create_dir_all(base.join("Textures"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("Textures/Tx_Wood.DDS"), "base")?;
    fs::write(base.join("Textures/Tx_Stone.dds"), "base")?;
    fs::write(patch.join("textures/tx_wood.dds"), "patch")?;

    // Later directories win, as with openmw.cfg's data= lines.
    let vfs = VFS::from_directories([&base, &patch], None);
    assert_eq!(vfs.len(), 2);

    let wood = vfs.get_file(r"TEXTURES\tx_wood.dds").unwrap();
    assert_eq!(wood.path(), patch.join("textures/tx_wood.dds"));
    assert!(vfs.contains("textures/tx_stone.dds"));

    let mut textures: Vec<&str> = vfs
        .paths_with("textures")
        .map(|(key, _)| key.to_str().unwrap())
        .collect();
    textures.sort_unstable();
    assert_eq!(textures, ["textures/tx_stone.dds", "textures/tx_wood.dds"]);

    fs::remove_dir_all(&root)
}
```

## VFS

{{ api_signature(value="struct VFS") }}

Fields are private. `Default` (the same as `new`) and [`Display`](#display). `Send` and `Sync`,
so one VFS can be read from many threads. Not `Clone`.

Keys are [`NormalizedPath`](@/docs/api/keys.md#normalizedpath) values: lowercase ASCII, `/`
separators, raw bytes with no UTF-8 requirement. Every lookup takes any
[`VfsKeyInput`](@/docs/api/keys.md#vfskeyinput), such as `&str`, `&Path`, `&[u8]` or a
`NormalizedPath`, and normalizes it first, so `Textures\Foo.DDS` and `textures/foo.dds` find the
same file.

Two rules hold for the keys of every `VFS`:

- **Every key can be written under a directory**: relative, not empty, no `.` or `..` component,
  no drive prefix, no NUL byte. Input that would break this is skipped or refused.
- **No key is both a file and a directory**: `meshes/door.nif` and `meshes/door.nif/x.nif` are
  never both present. Whichever arrives second is skipped or refused. One method can break this
  today; see [Known issues](#known-issues).

## Building one

{{ api_signature(value="fn new() -> VFS") }}

An empty VFS with no sources.

{{ api_signature(value="fn from_directories(search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>, archive_list: Option<Vec<&str>>) -> VFS") }}

Builds the VFS OpenMW would. `search_dirs` are the `data=` directories in `openmw.cfg` order;
`archive_list` holds the `fallback-archive=` names.

- Each directory is walked recursively, in parallel, following symbolic links. Every regular file
  becomes a provider under its path relative to the directory, normalized. A later directory's
  file wins over an earlier directory's.
- With the `beth-archives` or `zip` feature, each name in `archive_list` is looked up among the
  loose files like any other key, so `Morrowind.bsa` finds `Data Files/Morrowind.bsa`, and the copy
  in the highest-priority directory is the one opened. Every archive goes below every loose file,
  and a later archive in the list wins over an earlier one. Without either feature,
  `archive_list` is ignored.
- Two files in one directory that normalize to the same key, such as `Textures/A.dds` and
  `textures/a.dds` on a case-sensitive file system, are both kept as providers, and the one whose
  path sorts last, byte by byte, wins.

It does not fail. Whatever cannot be used is left out without a word: a directory that does not
exist or cannot be read, an entry the walk cannot read, an archive name that matches no loose file
or does not open, a key that is not safe, and a key that would make a file and a directory
collide. Sources are added archives first, then directories from lowest to highest priority, and
the collision check keeps whichever came first: a data directory's file `meshes/x` is dropped when a
lower-priority source already has `meshes/x/door.nif`. A caller that wants to know what was
skipped compares the result with its inputs; for `openmw.cfg`, the CLI's
[`validate`](@/docs/checking.md) does.

The provider-occurrence index is not built; [`layer_index`](#the-provider-index) builds it on first use.

{{ api_signature(value="fn from_directories_with_conflict_index(search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>, archive_list: Option<Vec<&str>>) -> (VFS, ConflictIndex)") }}

`from_directories`, and the [`ConflictIndex`](@/docs/api/conflicts.md) derived from the same
providers in one walk. The VFS keeps the [`LayerIndex`](@/docs/api/layer-index.md) it built on the
way, so `has_layer_index()` is `true`. In the conflict index, archives come first, from index 0,
then the directories.

{{ api_signature(value="fn from_directories_with_layer_index(search_dirs: impl IntoIterator<Item = impl AsRef<Path> + Sync>, archive_list: Option<Vec<&str>>) -> (VFS, LayerIndex)") }}

`from_directories`, and a copy of its [`LayerIndex`](@/docs/api/layer-index.md), which the VFS
keeps too. The index holds every key, including keys with a single provider.

{{ api_signature(value="fn push_directory<P: AsRef<Path>>(&mut self, root: P) -> std::io::Result<()>") }}

Adds every file under `root` as one new source, above everything already in the VFS. Unlike
`from_directories`, a walk error is returned rather than skipped, so a `root` that does not exist
is an error. An empty directory adds nothing, not even a source. Keys that are not safe, or that
would collide with a file or directory already present, are skipped.
[Changing a VFS](@/docs/api/mutation.md) explains how pushed sources stack.

{{ api_signature(value="fn push_archive<P: AsRef<Path>>(&mut self, archive_path: P) -> bool") }}

With `beth-archives` or `zip`. Opens one archive and adds its entries as one new source, above
everything already in the VFS, loose files included. That is the opposite of `from_directories`,
which puts archives at the bottom: `push_archive` is manual stacking. Returns `false`, changing
nothing, if the archive does not open, and `true` otherwise, even when none of its entries could
be added. [Archives](@/docs/api/archives.md) says which files open.

## Looking files up

{{ api_signature(value="fn get_file<P: VfsKeyInput + ?Sized>(&self, path: &P) -> Option<&VfsFile>") }}

The [`VfsFile`](@/docs/api/files.md) that wins `path`, or `None`. The key is normalized first; a
key already in normalized spelling is looked up as it is, without allocating. Keys name files,
not directories: `get_file("textures")` is `None` even when `textures/` has files in it.

{{ api_signature(value="fn get_file_normalized(&self, key: &[u8]) -> Option<&VfsFile>") }}

`get_file` for a key already in normalized spelling, with no normalization and no allocation. A
key in any other spelling is not found; debug builds assert. It is for loops that normalize into
their own buffer with dream-path:

```rust
use std::fs;

use dream_path::normalize_path_into;
use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-normalized");
    fs::create_dir_all(root.join("Meshes"))?;
    fs::write(root.join("Meshes/Door.NIF"), "nif")?;
    let vfs = VFS::from_directories([&root], None);

    let mut scratch = Vec::new();
    for spelling in [r"Meshes\Door.NIF", "meshes//door.nif", "MESHES/DOOR.NIF"] {
        normalize_path_into(&mut scratch, spelling.as_bytes());
        assert!(vfs.get_file_normalized(&scratch).is_some());
    }

    fs::remove_dir_all(&root)
}
```

That needs `dream-path = "1"` beside `vfstool_lib`.

{{ api_signature(value="fn contains<K: VfsKeyInput + ?Sized>(&self, key: &K) -> bool") }}

Whether `key` resolves to a file. Normalized like `get_file`.

{{ api_signature(value="fn contains_normalized(&self, key: &[u8]) -> bool") }}

`contains` for a key already in normalized spelling, like `get_file_normalized`.

{{ api_signature(value="fn len(&self) -> usize") }}

The number of resolved keys, in constant time. Losing providers do not count.

{{ api_signature(value="fn is_empty(&self) -> bool") }}

Whether no key resolves.

## Iterating

The winners live in a hash map. Everything in this section yields them in no particular order;
sort what you collect when order matters. Only resolved winners are yielded, never losing
providers.

{{ api_signature(value="fn iter(&self) -> impl Iterator<Item = (&NormalizedPath, &VfsFile)>") }}

Every key and the file that wins it.

{{ api_signature(value="fn par_iter(&self) -> impl ParallelIterator<Item = (&NormalizedPath, &VfsFile)>") }}

`iter` as a rayon parallel iterator. Using it takes `rayon` in your own dependencies, for the
`ParallelIterator` trait.

{{ api_signature(value="fn paths_with<P: VfsKeyInput + ?Sized>(&self, prefix: &P) -> impl Iterator<Item = (&NormalizedPath, &VfsFile)>") }}

Every key at or under the directory `prefix`, matched on whole path components: `textures` yields
`textures/foo.dds` and `textures/a/b.dds`, and not `textures2/foo.dds` or `textures.txt`. The
prefix is normalized and trailing separators are ignored, so `textures`, `textures/` and
`Textures\` are one prefix. A prefix that is itself a key yields that key. An empty prefix yields
nothing.

{{ api_signature(value="fn par_paths_with<P: VfsKeyInput + ?Sized>(&self, prefix: &P) -> impl ParallelIterator<Item = (&NormalizedPath, &VfsFile)>") }}

`paths_with` in parallel.

{{ api_signature(value="fn paths_matching<S: AsRef<str>>(&self, substring: S) -> impl Iterator<Item = (&NormalizedPath, &VfsFile)>") }}

Every key that contains `substring` anywhere. The substring is folded like a host path, `\` to `/`
and ASCII letters to lowercase, but repeated separators are kept. `Door` matches
`meshes/door.nif`; `TEXTURES` matches `textures/a.dds`, `textures2/b.dds` and `textures.txt`.

{{ api_signature(value="fn paths_matching_bytes(&self, substring: &[u8]) -> impl Iterator<Item = (&NormalizedPath, &VfsFile)>") }}

`paths_matching` for a needle that is not UTF-8. Folded the same way, compared byte for byte.

{{ api_signature(value="fn par_paths_matching<S: AsRef<str>>(&self, substring: S) -> impl ParallelIterator<Item = (&NormalizedPath, &VfsFile)>") }}

`paths_matching` in parallel.

## Searching into a tree

These two return a [`DisplayTree`](@/docs/api/materializing.md#trees), the sorted directory
tree the CLI prints and serializes. `relative` picks its shape: `true` roots it at `Data Files`
and lays files out by key; `false` roots it at `/` and lays them out by their real paths.
[Trees](@/docs/api/materializing.md#trees) has the details.

{{ api_signature(value="fn find_by_regex(&self, pattern: &str, relative: bool) -> std::result::Result<DisplayTree, regex::Error>") }}

The files whose key matches `pattern`, a [regex](https://docs.rs/regex) crate pattern compiled
case-insensitive and unanchored: `\.dds$` finds every DDS, `door` every key with `door` in it.
The key is matched as text, with bytes that are not UTF-8 read as U+FFFD. A pattern that does not
compile is the regex crate's error. `vfstool find` runs this.

{{ api_signature(value="fn remaining(&self, filter_path: &Path, replacements_only: bool, all_dirs: &[PathBuf], relative: bool) -> DisplayTree") }}

What is left of one data directory. `filter_path` is the directory, and `all_dirs` the full list
of data directories, from which the entries equal to `filter_path` are taken. Both comparisons are
on text, after `\` to `/` and ASCII lowercase
([`normalize_host_path`](@/docs/api/keys.md#normalize-host-path)).

- `replacements_only == false`: the files still served from `filter_path`, meaning every winner
  whose real path is at or under it. A data directory nested inside `filter_path` counts as part
  of it.
- `replacements_only == true`: the files `filter_path` has and loses, meaning every key a VFS of
  `filter_path` alone would contain whose winner is somewhere else. This needs `filter_path` in
  `all_dirs`; without it, the tree is empty.

Archive entries are never under a directory. `vfstool remaining` runs this.

```rust
use std::fs;
use std::path::Path;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-remaining");
    let base = root.join("base");
    let patch = root.join("patch");
    fs::create_dir_all(&base)?;
    fs::create_dir_all(&patch)?;
    fs::write(base.join("a.txt"), "base")?;
    fs::write(base.join("b.txt"), "base")?;
    fs::write(patch.join("a.txt"), "patch")?;

    let dirs = vec![base.clone(), patch.clone()];
    let vfs = VFS::from_directories(&dirs, None);
    let data_files = Path::new("Data Files");

    let still_served = vfs.remaining(&base, false, &dirs, true);
    let names: Vec<_> = still_served[data_files].files.iter().filter_map(|f| f.file_name()).collect();
    assert_eq!(names, ["b.txt"]);

    let replaced = vfs.remaining(&base, true, &dirs, true);
    let names: Vec<_> = replaced[data_files].files.iter().filter_map(|f| f.file_name()).collect();
    assert_eq!(names, ["a.txt"]);

    fs::remove_dir_all(&root)
}
```

`diff_directory`, which asks what a directory would change if it were added on top, is on
[ConflictIndex and reports](@/docs/api/conflicts.md).

## The provider index

{{ api_signature(value="fn layer_index(&self) -> &LayerIndex") }}

The [`LayerIndex`](@/docs/api/layer-index.md) of this VFS: every provider occurrence of every key,
by source. Built on the first call and kept; the constructors that return an index build it up
front, and every change to the VFS rebuilds it once it exists. Sources that no longer provide any
key are left out of it, so its source indices can be lower than the `source_index` in this VFS's
[provider records](@/docs/api/providers.md).

{{ api_signature(value="fn has_layer_index(&self) -> bool") }}

Whether the index exists yet: `false` after `new` and `from_directories`, `true` after the
`with_*_index` constructors or any call that needed it, such as `layer_index` or
`source_contributions`. A caller that only does lookups can check it is not paying for analysis.

## Display

`VFS` implements `Display` as the text tree of
[`display_filtered`](@/docs/api/materializing.md#trees) with every file, in relative form. For a
data directory holding `Morrowind.esm` and `Meshes/X/Door.NIF`:

```text
├── Data Files/
│   ├── Morrowind.esm
├── x/
│   ├── Door.NIF
```

## Known issues

`push_provider_batch` checks its entries against the VFS as it was, not against each other, so a
batch can leave a VFS with a file and a directory under one name; see
[Adding](@/docs/api/mutation.md#adding).
