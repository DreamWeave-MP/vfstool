+++
title = "Provider reports"
description = "What a VFS can say about its own provider stacks: provider records, explain, duplicates, archives and their entries, and per-source contribution counts, with every report type's fields."
weight = 30

[extra]
kind = "api"
+++

A [`VFS`](@/docs/api/vfs.md) keeps every provider of every key, so it can say why a key resolves
the way it does, which keys more than one source provides, and what each source and archive
contributes. These are the calls behind `vfstool explain`, `duplicates`, `archives`,
`archive-list` and `contributions`. Every report is plain data: `Debug`, `Clone`, and with the
`serialize` feature `Serialize`, with the field names below.

```rust
use std::fs;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-providers");
    let base = root.join("base");
    let patch = root.join("patch");
    fs::create_dir_all(base.join("Textures"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("Textures/Tx_Wood.DDS"), "base")?;
    fs::write(base.join("Textures/Tx_Stone.dds"), "base")?;
    fs::write(patch.join("textures/tx_wood.dds"), "patch")?;

    let vfs = VFS::from_directories([&base, &patch], None);

    let report = vfs.explain("textures/tx_wood.dds").unwrap();
    assert_eq!(report.winner.source.path, patch);
    assert_eq!(report.overridden.len(), 1);
    assert_eq!(report.overridden[0].original_path.to_str(), Some("Textures/Tx_Wood.DDS"));

    let duplicates = vfs.duplicates();
    assert_eq!(duplicates.entries.len(), 1);
    assert_eq!(duplicates.entries[0].winner_index, 1);

    for row in vfs.source_contributions().sources {
        println!("{}: {} winning, {} overridden", row.source.path.display(), row.winning_files, row.overridden_files);
    }

    fs::remove_dir_all(&root)
}
```

## Provider records

{{ api_signature(value="fn provider_records_for<K: crate::VfsKeyInput + ?Sized>(&self, path: &K) -> Vec<VfsProviderRecord>") }}

Every provider of `path` as a report row, lowest priority first; the last is the winner. Empty
when the key has no providers.

{{ api_signature(value="struct VfsProviderRecord") }}

| Field | Type | Meaning |
|---|---|---|
| `source_index` | `usize` | The source's position in the VFS, from 0, lowest priority first. It counts every source ever added, so it stays the same when providers are removed |
| `source` | [`SourceMeta`](@/docs/api/layer-index.md) | The source's path and kind |
| `key` | `PathBuf` | The normalized key |
| `original_path` | `PathBuf` | The name as the source spells it: for a loose file, its path relative to the source directory, in its own case; for an archive entry, the entry's path as stored |
| `resolved_path` | `String` | Where the bytes are: the source directory joined with `original_path`, or for an archive `ARCHIVE::ENTRY`, such as `/games/Data Files/Morrowind.bsa::meshes\x\ex_door.nif` |

A loose file that is not under its source's directory, which only a hand-built provider passed to
[`push_provider`](@/docs/api/mutation.md#adding) can make, gets the key as its `original_path`. A
file set with `set_winner_file` has its own directory as its source, so its `original_path` is its
file name.

## Explain

{{ api_signature(value="fn explain<K: crate::VfsKeyInput + ?Sized>(&self, path: &K) -> Option<ExplainReport>") }}

Why `path` resolves as it does: the winner and everything it overrides. `None` when the key has
no providers. `vfstool explain` prints this.

{{ api_signature(value="struct ExplainReport") }}

| Field | Type | Meaning |
|---|---|---|
| `key` | `PathBuf` | The normalized key |
| `winner` | `VfsProviderRecord` | The provider that wins |
| `overridden` | `Vec<VfsProviderRecord>` | The providers it overrides, lowest priority first. Empty for a key with one provider |

## Duplicates

{{ api_signature(value="fn duplicates(&self) -> DuplicateReport") }}

Every key with more than one provider, sorted by key. Two spellings of one name in the same source
count: that key has two providers.

{{ api_signature(value="fn duplicates_matching_regex(&self, pattern: &str) -> std::result::Result<DuplicateReport, regex::Error>") }}

`duplicates`, keeping only keys that match `pattern`: a [regex](https://docs.rs/regex) pattern,
case-insensitive and unanchored, matched against the normalized key as text, never against source
paths. `^meshes/` keeps meshes, `\.dds$` keeps DDS files. Returns the regex crate's error for a
pattern that does not compile.

{{ api_signature(value="struct DuplicateReport") }}

| Field | Type | Meaning |
|---|---|---|
| `entries` | `Vec<DuplicateEntry>` | One per key, sorted by key |

{{ api_signature(value="struct DuplicateEntry") }}

| Field | Type | Meaning |
|---|---|---|
| `key` | `PathBuf` | The normalized key |
| `providers` | `Vec<VfsProviderRecord>` | Every provider, lowest priority first |
| `winner_index` | `usize` | The index in `providers` of the winner, which is always the last |

## Archives

These work without an archive feature, and then return nothing: archives are only loaded with
`beth-archives` or `zip`.

{{ api_signature(value="fn archives(&self) -> Vec<ArchiveInfo>") }}

Every archive source in the VFS, sorted by path, including an archive none of whose entries
were kept.

{{ api_signature(value="struct ArchiveInfo") }}

| Field | Type | Meaning |
|---|---|---|
| `source_index` | `usize` | The archive's source position, as in `VfsProviderRecord` |
| `path` | `PathBuf` | The archive file, as it was opened |
| `entry_count` | `usize` | How many providers the archive has in the VFS: its entries, less any skipped as unsafe or colliding |
| `winning_entry_count` | `usize` | How many of those win their key |

{{ api_signature(value="fn archive_entries(&self, archive: impl AsRef<Path>) -> Vec<ArchiveEntry>") }}

Every provider an archive supplies, winning or not, sorted by key, then archive path, original
path and `wins`. `archive` is the archive's full path, compared as text after `\` to `/` and ASCII
lowercase, so `.../data files/morrowind.bsa` finds `.../Data Files/Morrowind.bsa`, but a bare file
name finds nothing. `vfstool archive-list` resolves a file name, a path suffix or a source index
to the full path first.

{{ api_signature(value="struct ArchiveEntry") }}

| Field | Type | Meaning |
|---|---|---|
| `key` | `PathBuf` | The normalized key |
| `archive_path` | `PathBuf` | The archive file |
| `original_path` | `PathBuf` | The entry's path as the archive stores it |
| `wins` | `bool` | Whether this entry is the key's winner |

{{ api_signature(value="fn files_from_archive(&self, archive: impl AsRef<Path>) -> Vec<PathBuf>") }}

The keys of `archive_entries`, in the same order. An archive holding two entries that normalize to
one key lists that key twice.

## Contributions

{{ api_signature(value="fn source_contributions(&self) -> SourceContributionReport") }}

What each source contributes, counted over the [`LayerIndex`](@/docs/api/layer-index.md), which
this builds if the VFS does not have it yet. `vfstool contributions` prints it. `LayerIndex` has a
method of the same name that returns the same report.

{{ api_signature(value="struct SourceContributionReport") }}

| Field | Type | Meaning |
|---|---|---|
| `sources` | `Vec<SourceContribution>` | One row per source, lowest priority first |

{{ api_signature(value="struct SourceContribution") }}

Every count is of provider occurrences, not distinct keys: a source that has two spellings of one
name counts twice. Two occurrences from the same source never count as overriding each other.

| Field | Type | Counts the occurrences |
|---|---|---|
| `source_index` | `usize` | The source's position in the index, lowest priority first |
| `source` | [`SourceMeta`](@/docs/api/layer-index.md) | The source's path and kind |
| `winning_files` | `usize` | that are their key's winner |
| `overriding_files` | `usize` | that have a provider from another source below them |
| `overridden_files` | `usize` | that are not the winner and have a provider from another source above them |
| `unique_files` | `usize` | whose key has no other provider |
| `duplicate_files` | `usize` | whose key has another provider, from any source |
| `loose_files` | `usize` | from a loose directory: all of them, or none |
| `archive_files` | `usize` | from an archive: all of them, or none |

The index only has sources that provide at least one key. A data directory that is empty or
missing has no row, and the `source_index` here then differs from the one in
`VfsProviderRecord` and `ArchiveInfo`, which count every source: with an empty first directory,
the second directory is `source_index` 1 in `explain` and 0 here.
