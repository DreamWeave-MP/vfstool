+++
title = "Changing a VFS"
description = "VfsProvider, and every method that adds or removes providers: winner-only changes that replace or discard a key's whole stack, and stack changes that reveal what was underneath."
weight = 20

[extra]
kind = "api"
+++

Every key in a [`VFS`](@/docs/api/vfs.md) has a provider stack, lowest priority first, and the
key resolves to the top of it. There are two kinds of change, and the method names say which:

- **Winner-only**: `set_winner_file`, `set_winner_loose_file` and the `remove_resolved_*`
  methods replace or discard a key's whole stack. Nothing underneath survives.
- **Stack changes**: `push_*` put one provider or one source on top; `remove_winner`,
  `remove_provider`, `remove_source` and `remove_matching_provider` take providers out, and
  whatever was below the removed winner becomes the winner.

```rust
use std::fs;
use std::path::Path;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-mutation");
    let base = root.join("base");
    let patch = root.join("patch");
    fs::create_dir_all(&base)?;
    fs::create_dir_all(&patch)?;
    fs::write(base.join("door.nif"), "base")?;
    fs::write(patch.join("door.nif"), "patch")?;

    let mut vfs = VFS::from_directories([&base, &patch], None);
    assert_eq!(vfs.get_file("door.nif").unwrap().path(), patch.join("door.nif"));

    // A stack change: the patch's copy goes, the base's copy is revealed.
    let removed = vfs.remove_winner("door.nif").unwrap();
    assert_eq!(removed.source.path, patch);
    assert_eq!(vfs.get_file("door.nif").unwrap().path(), base.join("door.nif"));

    // A winner-only change: one provider replaces the whole stack.
    vfs.set_winner_loose_file("door.nif", Path::new("/elsewhere/door.nif"));
    assert_eq!(vfs.providers_for("door.nif").unwrap().len(), 1);

    // Nothing is left underneath to reveal.
    vfs.remove_winner("door.nif");
    assert!(!vfs.contains("door.nif"));

    fs::remove_dir_all(&root)
}
```

## How changes behave

**Sources.** A VFS keeps a list of sources, in the order they were added. `from_directories` adds
one per archive and one per directory; after that, every `push_*` call that adds a provider, and
every `set_winner_file`, adds one more on top, even when its path equals an existing source's path.
Removing providers never removes a source, so the `source_index` in
[provider records](@/docs/api/providers.md) stays stable across changes.

**Keys.** Keys are normalized as for [lookups](@/docs/api/vfs.md#looking-files-up). Methods that
add a key refuse one that is not safe to write (absolute, empty, `..`, a drive prefix, NUL) and a
new key that would make a file and a directory collide.

**Prefixes.** The `*_prefix` methods match whole path components after normalization, with
trailing separators ignored: `textures` covers `textures/foo.dds` and `textures` itself, not
`textures2/foo.dds`.

**Source paths.** `remove_provider` and `remove_source` compare source paths as `Path` values:
component by component, case-sensitive, with no canonicalization. `/mods/Patch/` matches a source
added as `/mods/Patch`, and `/mods/patch` does not. Pass the path the way the source was added.

**The index.** When the VFS holds a [`LayerIndex`](@/docs/api/layer-index.md), every change that
alters a provider rebuilds it in full. `push_provider_batch` rebuilds it once for the whole batch,
where the same number of `push_provider` calls rebuild it once each.

## VfsProvider

{{ api_signature(value="struct VfsProvider") }}

One provider of one key: where it comes from and the file behind it. `Debug`, `Clone`. Marked
`#[non_exhaustive]`, so it is built with `new` outside the crate, and its fields are public to
read.

| Field | Type | Meaning |
|---|---|---|
| `source` | [`SourceMeta`](@/docs/api/layer-index.md) | The source's path and kind, `LooseDir` or `Archive` |
| `file` | [`VfsFile`](@/docs/api/files.md) | The file that provides the key |

{{ api_signature(value="fn new(source: SourceMeta, file: VfsFile) -> VfsProvider") }}

## Reading a stack

{{ api_signature(value="fn providers_for<K: VfsKeyInput + ?Sized>(&self, key: &K) -> Option<impl ExactSizeIterator + DoubleEndedIterator<Item = &VfsProvider>>") }}

The providers of `key`, lowest priority first, so `next_back()` is the winner. `None` when the key
has none. [Provider reports](@/docs/api/providers.md) has the same stack as report rows.

## Adding

{{ api_signature(value="fn set_winner_file<K: VfsKeyInput + ?Sized>(&mut self, key: &K, file: VfsFile) -> Option<VfsFile>") }}

Makes `file` the only provider of `key`, discarding the stack. The new provider gets a source of
its own: the archive's path for an archive entry, and the file's parent directory for a loose
file. Returns the file that was the winner before.

`None` means either that the key was new or that it was refused, being unsafe or colliding with a
file or directory already there; check `contains(key)` to tell them apart. A new key's directories
are recorded, so a later file by one of their names is refused.

{{ api_signature(value="fn set_winner_loose_file<K: VfsKeyInput + ?Sized, P: AsRef<Path>>(&mut self, key: &K, physical_path: P) -> Option<VfsFile>") }}

`set_winner_file(key, VfsFile::from(physical_path))`. The path is not checked; a file that does
not exist shows up later, when something reads it.

{{ api_signature(value="fn push_provider<K: VfsKeyInput + ?Sized>(&mut self, key: &K, provider: VfsProvider) -> bool") }}

Puts `provider` on top of `key`'s stack, as a new source, so it wins. Returns `false`, changing
nothing, when the key is unsafe or would collide. Each call is a source of its own; to add many
files as one source, use `push_provider_batch` or `push_directory`.

{{ api_signature(value="fn push_provider_batch(&mut self, source: &SourceMeta, entries: impl IntoIterator<Item = (NormalizedPath, VfsFile)>) -> usize") }}

Adds every entry as a provider from one new source, on top, and returns how many were added
(`#[must_use]`). Keys are taken as they are, so an entry built with `NormalizedPath::new` is
normalized already; entries whose keys are unsafe are dropped, and so are new keys that collide
with what the VFS already had. Two entries with the same key both go on the stack, the later one
winning. When nothing is added, no source is added either.

Entries are added in order, each checked against the VFS and the entries before it: of a batch
holding both `a` and `a/b.txt`, the first is added and the second dropped. `push_directory` and
`push_archive` go through this method; `push_directory` sorts a directory's files by key first, as
`from_directories` does, so both keep the same one of a file and a directory with one name.

`push_directory` and `push_archive`, which add a whole directory or archive as one source, are
under [Building one](@/docs/api/vfs.md#building-one).

## Removing

Every method here returns what it removed, in no particular key order, and leaves any key whose
stack becomes empty unresolved.

{{ api_signature(value="fn remove_winner<K: VfsKeyInput + ?Sized>(&mut self, key: &K) -> Option<VfsProvider>") }}

Takes the top provider off `key`'s stack. The next one down, if there is one, becomes the winner.
`None` when the key has no providers.

{{ api_signature(value="fn remove_resolved_file<K: VfsKeyInput + ?Sized>(&mut self, key: &K) -> Option<VfsFile>") }}

Removes `key` and its whole stack, returning the file that was winning. `None` when the key did
not resolve.

{{ api_signature(value="fn remove_provider<K: VfsKeyInput + ?Sized>(&mut self, key: &K, source: &Path) -> Vec<VfsProvider>") }}

Removes every provider of `key` whose source path is `source`, lowest first. A source can provide
one key more than once, from two spellings of one name. The rest of the stack stays, and its top
wins.

{{ api_signature(value="fn remove_source(&mut self, source: &Path) -> Vec<(NormalizedPath, VfsProvider)>") }}

Removes every provider whose source path is `source`, from every key. Where that source was
winning, the next provider down wins. This is taking a mod out of the load order.

{{ api_signature(value="fn remove_provider_prefix<K: VfsKeyInput + ?Sized>(&mut self, prefix: &K) -> Vec<(NormalizedPath, VfsProvider)>") }}

Removes every provider, from every source, of every key at or under `prefix`, so all of those keys
stop resolving. It differs from `remove_resolved_prefix` in what it returns: every provider, not
just the winners.

{{ api_signature(value="fn remove_resolved_prefix<K: VfsKeyInput + ?Sized>(&mut self, prefix: &K) -> Vec<(NormalizedPath, VfsFile)>") }}

Removes every key at or under `prefix` and its stack, returning the files that were winning.

{{ api_signature(value="fn remove_matching_provider(&mut self, matcher: impl FnMut(&NormalizedPath, &VfsProvider) -> bool) -> Vec<(NormalizedPath, VfsProvider)>") }}

Calls `matcher` on every provider of every key and removes the ones it accepts. Where the winner
is removed and a provider below it is kept, that one wins. `remove_source` is this with a matcher
on the source path.

{{ api_signature(value="fn remove_resolved_matching_glob(&mut self, glob: &str) -> Vec<(NormalizedPath, VfsFile)>") }}

Removes every key matching `glob` and its stack, returning the files that were winning. The glob
syntax is [`path_glob_matches`](@/docs/api/keys.md#path-glob-matches)'s: `**/*.dds` removes every
DDS.

```rust
use std::fs;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-remove-source");
    let base = root.join("base");
    let patch = root.join("patch");
    fs::create_dir_all(base.join("textures"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("textures/wood.dds"), "base")?;
    fs::write(patch.join("textures/wood.dds"), "patch")?;
    fs::write(patch.join("textures/new.dds"), "patch")?;

    let mut vfs = VFS::from_directories([&base, &patch], None);

    // Take the patch out of the load order.
    let removed = vfs.remove_source(&patch);
    assert_eq!(removed.len(), 2);
    assert_eq!(vfs.get_file("textures/wood.dds").unwrap().path(), base.join("textures/wood.dds"));
    assert!(!vfs.contains("textures/new.dds"));

    // Discard every DDS, whoever provides it.
    let gone = vfs.remove_resolved_matching_glob("**/*.dds");
    assert_eq!(gone.len(), 1);
    assert!(vfs.is_empty());

    fs::remove_dir_all(&root)
}
```
