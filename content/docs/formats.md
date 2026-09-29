+++
title = "Report formats"
description = "The fields of every report vfstool writes, as YAML, JSON or TOML: trees, provider records, conflicts, contributions, validation issues, locks, drift and collapse plans."
weight = 65

[extra]
kind = "reference"
+++

Every report is one of the structures below, written by serde as YAML (the default), JSON (`-f
json`, compact) or TOML (`-f toml`). Field names are `snake_case` in all three. The
[Rust API](@/docs/api/_index.md) has the same structures as types.

- **Paths** are strings. A key is lowercase with `/`; a path on disk is as the cfg or the file
  system spells it.
- **Source kinds** are `loose_dir` or `archive`.
- **Variants**, in `validate` and collapse plans, are tagged. YAML writes the tag before the
  fields (`- !MissingContentFile`), JSON as an object with one key
  (`{"MissingContentFile":{"name":"Missing Plugin.esp"}}`), TOML as a table of that name.
- **Lists** at the top level cannot be TOML, so in TOML the list `archives` writes is under the
  key `archives`, and the one `archive-list` writes under `entries`.

On Linux and macOS a file name need not be UTF-8. YAML, JSON and TOML hold only text, so such a
name is written with U+FFFD (`�`) in place of each byte sequence that is not UTF-8. `drift`
compares a lock the same way, so a lock reads back without drift from the files it was made from.

## Trees: find, remaining

A map from folder name to folder, nested. A folder's own files are a list under the key `.`,
sorted by name; its subfolders are keys beside it. The root is `/`, or with `-r`, `Data Files`.

```yaml
Data Files:
  .:
  - Lantern Glow.esp
  meshes:
    l:
      .:
      - Light_Com_Lantern_01.NIF
```

Loose files are listed by their name on disk; archive entries, on Linux and macOS, by their whole
name inside the archive.

## Provider records: explain, duplicates

`explain` writes one key's providers:

| Field | |
|---|---|
| `key` | The key |
| `winner` | The provider OpenMW loads |
| `overridden` | The other providers, lowest priority first |

`duplicates` writes `entries`, one per key with more than one provider, sorted by key:

| Field | |
|---|---|
| `key` | The key |
| `providers` | Every provider, lowest priority first |
| `winner_index` | The winner's position in `providers`, from 0 |

Each provider:

| Field | |
|---|---|
| `source_index` | The source's number in load order, from 0: archives first, then data directories |
| `source.path` | The data directory or archive |
| `source.kind` | `loose_dir` or `archive` |
| `key` | The key |
| `original_path` | The name as the source spells it: the path under the data directory, or the name in the archive |
| `resolved_path` | For a loose file, its path on disk; for an archive entry, `<archive>::<name in archive>` |

## Archives: archives, archive-list

`archives` writes a list, one per loaded archive, under `archives` in TOML:

| Field | |
|---|---|
| `source_index` | The archive's source number, for `archive-list --source-index` |
| `path` | The archive file |
| `entry_count` | Files in it |
| `winning_entry_count` | Files in it that win |

`archive-list` writes a list, one per file in the archive, sorted by key, under `entries` in TOML:

| Field | |
|---|---|
| `key` | The key |
| `archive_path` | The archive file |
| `original_path` | The name in the archive |
| `wins` | Whether this copy is the one OpenMW loads |

## conflicts

`sources`, every source with files, in load order:

| Field | |
|---|---|
| `path` | The data directory or archive |
| `overrides` | Its files that a lower source also has |
| `overridden_by` | Its files that a higher source also has |

Files are keys with `-r`. Without it, a data directory's files are their paths on disk, spelled as
the files are, and an archive's are `<archive>::<key>`.

## shadowed

`sources`, the sources whose every file a higher source overrides:

| Field | |
|---|---|
| `path` | The data directory or archive |
| `shadowed_files` | With `--list-files`, its files, spelled as in `conflicts`; otherwise empty |

## contributions

`sources`, every source, in load order, an empty or missing data directory too:

| Field | |
|---|---|
| `source_index` | The source's number in load order, as in `explain` and `archives` |
| `source` | As in a provider record |
| `winning_files` | Its files that win |
| `overriding_files` | Its files that a lower source also has |
| `overridden_files` | Its files that a higher source also has |
| `unique_files` | Its files no other source has |
| `duplicate_files` | Its files another source also has |
| `loose_files`, `archive_files` | Its files, counted by the kind of source |

## diff

| Field | |
|---|---|
| `source_a`, `source_b` | The two directories, as the cfg spells them |
| `higher_priority` | The later of the two in load order |
| `shared` | Keys both have, sorted |
| `only_in_a`, `only_in_b` | Keys only one has, sorted |

## validate

`issues`, a list of tagged variants: data directories first, then archives, content files and
groundcover files, each in cfg order.

| Variant | Fields |
|---|---|
| `MissingDataDirectory` | `path` |
| `DataPathNotDirectory` | `path` |
| `MissingFallbackArchive` | `name` |
| `UnreadableFallbackArchive` | `name`, and `path`, the file that could not be opened. `--full` only |
| `MissingContentFile` | `name` |
| `MissingGroundcoverFile` | `name` |

## lock

| Field | |
|---|---|
| `schema_version` | 1 |
| `entries` | One per key in the VFS, sorted by key |

Each entry:

| Field | |
|---|---|
| `key` | The key |
| `winner_source` | The data directory or archive the winning copy comes from |
| `winner_kind` | `loose_dir` or `archive` |
| `winner_hash_blake3` | The BLAKE3 hash of the winning copy, in hex |
| `winner_size` | Its size in bytes |
| `provider_count` | How many sources have the key |

## drift

| Field | |
|---|---|
| `entries` | One per key and kind of change, sorted by key, then kind |
| `counts` | The number of entries of each kind that occurs |

Each entry has a `key` and a `kind`: `added`, `removed`, `winner_source_changed`,
`winner_hash_changed` or `provider_count_changed`.
[Validating and locking](@/docs/checking.md#find-what-changed) says what each means.

## collapse --dry-run

`actions`, one per file, sorted by key, then `issues`. Actions:

| Variant | Fields | Is |
|---|---|---|
| `hardlink` | `key`, `source`, `dest` | A loose file, hardlinked |
| `symlink` | `key`, `source`, `dest` | A loose file, with `-s` |
| `copy` | `key`, `source`, `dest` | Never planned: a copy happens only when a link fails |
| `extract_archive` | `key`, `archive`, `dest` | A file from inside an archive, with `-e` |
| `skip_archive_file` | `key`, `archive` | A file from inside an archive, without `-e`; with `-e`, an archive file itself |

Issues:

| Variant | Fields | Is |
|---|---|---|
| `missing_loose_source` | `key`, `source` | The loose file is gone since the VFS was built |
| `file_directory_conflict` | `key`, `dest` | A file and a folder at one path. A VFS built from a cfg never has one |
| `unsafe_destination` | `key`, `dest` | The key cannot be a path here, or a folder on the way is a symbolic link |
