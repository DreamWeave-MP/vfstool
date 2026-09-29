+++
title = "Report tables"
description = "Every table the module returns, field by field: sources, providers, explain, duplicates, archives, contributions, materialization plans, directory diffs, provenance, locks, drift, semantic conflicts, and the conflict reports."
weight = 60

[extra]
kind = "api"
+++

Reports are plain Luau tables, built fresh for each call; changing one changes nothing else. Field
names are the Rust fields in camelCase, and so are the strings that stand for Rust enums. Indices
count from 1, byte sizes are integers, and every path is a byte string. A field shown as `T?` is
`nil` when there is nothing to say. The Rust structs behind them are on the [Rust
API](@/docs/api/_index.md) pages.

## Source

The data directory or archive a file comes from.

| Field | Type | Meaning |
|---|---|---|
| `path` | `string` | The directory as it was given, or the archive's host path |
| `kind` | `string` | `"looseDir"` or `"archive"` |

## Provider

A row of a `Providers` view, and what `vfs:removeProvider()` returns.

| Field | Type | Meaning |
|---|---|---|
| `source` | `Source` | Where the file comes from |
| `file` | `VfsFile` | The file |

`vfs:removeSource()` and `vfs:removeProviderPrefix()` return `RemovedProvider` rows, `{ key,
provider }`, with `provider` a `Provider`. `vfs:removeResolvedPrefix()` and
`vfs:removeResolvedMatchingGlob()` return `RemovedFile` rows, `{ key, file }`.

## Provider records

One provider of a key, as a report row: a row of a `ProviderRecords` view, and the rows of
`explain` and `duplicates`.

| Field | Type | Meaning |
|---|---|---|
| `sourceIndex` | `number` | The source's position among the VFS's sources, from 1 |
| `source` | `Source` | The source |
| `key` | `string` | The normalized key |
| `originalPath` | `string` | The key as the source spells it: `Meshes/x/Ex_Door.nif` |
| `resolvedPath` | `string` | Where the file is: the source path joined with `originalPath` for a loose file, `archive::originalPath` for an archive entry |

## Explain

`vfs:explain()`.

| Field | Type | Meaning |
|---|---|---|
| `key` | `string` | The normalized key |
| `winner` | provider record | The provider that wins |
| `overridden` | `{ provider record }` | The providers it overrides, lowest priority first |

## Duplicates

`vfs:duplicates()`: `{ entries = { ... } }`, one entry per key with more than one provider,
sorted by key.

| Field | Type | Meaning |
|---|---|---|
| `key` | `string` | The key |
| `providers` | `{ provider record }` | Every provider, lowest priority first |
| `winnerIndex` | `number` | The winner's position in `providers`: always the last |

## Archives

`vfs:archives()`: an array of `ArchiveInfo`, sorted by path.

| Field | Type | Meaning |
|---|---|---|
| `sourceIndex` | `number` | The archive's position among the VFS's sources |
| `path` | `string` | The archive's host path |
| `entryCount` | `number` | How many keys it provides |
| `winningEntryCount` | `number` | How many of those it wins |

## Archive entries

`vfs:archiveEntries()`: an array of `ArchiveEntry`, sorted by key.

| Field | Type | Meaning |
|---|---|---|
| `key` | `string` | The normalized key |
| `archivePath` | `string` | The archive's host path |
| `originalPath` | `string` | The entry's name inside the archive |
| `wins` | `boolean` | Whether this entry is the key's winner |

## Contributions

`vfs:sourceContributions()` and `layer:sourceContributions()`: `{ sources = { ... } }`, one row per
source of the `LayerIndex`, in load order. The counts are of provider occurrences: a source that
provides one key twice, as two spellings in one archive, counts twice.

| Field | Type | Meaning |
|---|---|---|
| `sourceIndex` | `number` | The source's position in the `LayerIndex` |
| `source` | `Source` | The source |
| `winningFiles` | `number` | Files it provides that win |
| `overridingFiles` | `number` | Files it provides over another source's copy |
| `overriddenFiles` | `number` | Files it provides that another source overrides |
| `uniqueFiles` | `number` | Files no other provider has |
| `duplicateFiles` | `number` | Files another provider also has |
| `looseFiles` | `number` | Files it provides as loose files |
| `archiveFiles` | `number` | Files it provides from an archive |

A second copy from the same source does not count as overriding or overridden.

## Materialization plan

`vfs:materializationPlan()`: `{ actions = { ... }, issues = { ... } }`. Every row has a `kind` and
a `key`, and the path fields its kind uses; the others are absent.

| Action `kind` | Fields | What `collapseInto` would do |
|---|---|---|
| `"hardlink"` | `source`, `dest` | Hard-link the loose file at `source` to `dest` |
| `"symlink"` | `source`, `dest` | Link it symbolically |
| `"copy"` | `source`, `dest` | Copy it |
| `"extractArchive"` | `archive`, `dest` | Extract the entry from `archive` to `dest` |
| `"skipArchiveFile"` | `archive` | Leave the key out: an archive entry without `extractArchives`, or, with it, the archive file itself |

| Issue `kind` | Fields | Problem |
|---|---|---|
| `"missingLooseSource"` | `source` | The loose file no longer exists |
| `"unsafeDestination"` | `dest` | The destination, or a directory on the way to `dest`, is a symbolic link |
| `"fileDirectoryConflict"` | `dest` | A file and a directory would need the same path |

A key with an issue has no action. The planner cannot know in advance whether a link will fail,
so `allowCopying` does not change the plan and `"copy"` does not appear in it; and a VFS never
holds a key that is both a file and a directory, so neither does `"fileDirectoryConflict"`. Both
kinds are part of the shape for the Rust type's sake.

## Directory diff

`vfs:diffDirectory()`, in no particular order.

| Field | Type | Meaning |
|---|---|---|
| `conflicts` | `{ { key: string, incoming: VfsFile, current: VfsFile } }` | Keys the directory would take over, with its file and the current winner |
| `additions` | `{ { key: string, file: VfsFile } }` | Keys the directory would add |

## Layer providers

`layer:providerChain()`: one row per provider of a key, lowest priority first.

| Field | Type | Meaning |
|---|---|---|
| `sourceIndex` | `number` | The source's position in the `LayerIndex` |
| `providerIndex` | `number` | The provider's position in this chain |
| `source` | `Source` | The source |
| `key` | `string` | The normalized key |
| `originalPath` | `string` | The key as the source spells it |

## Provenance

`layer:provenance()`.

| Field | Type | Meaning |
|---|---|---|
| `key` | `string` | The normalized key |
| `winner` | `Source` | The winning source |
| `providers` | `{ ... }` | Every provider, lowest priority first, with the fields below |

| Provider field | Type | Meaning |
|---|---|---|
| `source` | `Source` | The source |
| `resolvedPath` | `string` | As in a provider record |
| `hashBlake3` | `string?` | The content's BLAKE3 hash, 64 hex digits, when asked for and readable |
| `size` | `integer?` | The content's size in bytes, with the hash |

## Lock entries

`lock:entries()`, sorted by key, and the `entries` of `lock:toTable()`, whose other field is
`schemaVersion`.

| Field | Type | Meaning |
|---|---|---|
| `key` | `string` | The normalized key |
| `winnerSource` | `string` | The winning source's path |
| `winnerKind` | `string` | `"looseDir"` or `"archive"` |
| `winnerHashBlake3` | `string?` | The winning content's BLAKE3 hash |
| `winnerSize` | `integer?` | Its size in bytes |
| `providerCount` | `number` | How many providers the key has |

## Drift

`layer:diffAgainstLock()`.

| Field | Type | Meaning |
|---|---|---|
| `entries` | `{ { key: string, kind: string } }` | One row per key and change, sorted by key, then in the order below |
| `counts` | `{ [string]: number }` | How many rows of each kind; kinds with none are absent |

| `kind` | The key |
|---|---|
| `"added"` | Is not in the lock |
| `"removed"` | Is in the lock but no longer resolves |
| `"winnerSourceChanged"` | Is won by another source |
| `"winnerHashChanged"` | Has different winning content |
| `"providerCountChanged"` | Has more or fewer providers |

## Semantic conflicts

`layer:semanticConflicts()`: `{ entries = { ... } }`, one entry per key with more than one
provider, sorted by key.

| Field | Type | Meaning |
|---|---|---|
| `key` | `string` | The normalized key |
| `winner` | `Source` | The winning source |
| `providers` | `{ ... }` | Every provider, lowest priority first, with the fields below |
| `assetClass` | `string` | As for [`analyzePair`](@/docs/luau/module.md#analyzepair) |
| `allIdentical` | `boolean` | Whether every provider has the winner's content |
| `distinctVersions` | `number` | How many different contents the providers have |

| Provider field | Type | Meaning |
|---|---|---|
| `source` | `Source` | The source |
| `relation` | `string` | `"identicalToWinner"`, `"differentFromWinner"`, or `"unknown"` when it was not read |
| `hashBlake3` | `string?` | The content's BLAKE3 hash, when read |
| `size` | `integer?` | Its size in bytes, when read |
| `semanticDeltaToWinner` | `{ kind: string, changeSummary: { string }? }?` | With `includeSemanticDeltas`: how it differs from the winner, as for `analyzePair` |

## Conflict reports

`conflicts:conflictsReport()`: `{ sources = { ... } }`, one row per source in load order.

| Field | Type | Meaning |
|---|---|---|
| `path` | `string` | The source |
| `overrides` | `{ string }` | Paths it provides over a lower source, sorted |
| `overriddenBy` | `{ string }` | Paths a higher source provides over it, sorted |

`conflicts:shadowedReport()`: `{ sources = { ... } }`, one row per source that loses every file
it provides.

| Field | Type | Meaning |
|---|---|---|
| `path` | `string` | The source |
| `shadowedFiles` | `{ string }` | Its files, sorted; empty when `listFiles` is `false` |

`conflicts:diffReport()`:

| Field | Type | Meaning |
|---|---|---|
| `sourceA`, `sourceB` | `string` | The two sources, as given |
| `higherPriority` | `string` | The one that wins where they overlap |
| `shared` | `{ string }` | Keys both have, sorted |
| `onlyInA`, `onlyInB` | `{ string }` | Keys only one has, sorted |

## analyzePair and runFinalize

`analyzePair()` returns `{ assetClass: string, delta: { kind: string, changeSummary: { string }? } }`,
described [with the function](@/docs/luau/module.md#analyzepair). `runFinalize()` and
`runFinalizeTracked()` return `Copied` rows:

| Field | Type | Meaning |
|---|---|---|
| `relativePath` | `string` | The file's path under the merged directory |
| `destinationPath` | `string` | Where it was copied to |

```lua
local vfstool = require("@dream/vfs")

local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex(
    { "Data Files", "mods/Wood Retexture", "mods/Door Fix" },
    { archives = { "Extras.zip" } }
)

local archiveRow = vfs:sourceContributions().sources[1]
assert(archiveRow.source.kind == "archive" and archiveRow.archiveFiles == 2)
assert(archiveRow.winningFiles == 1 and archiveRow.overriddenFiles == 1)

local provenance = layer:provenance(vfs, "meshes/x/ex_door.nif", true)
assert(provenance.winner.path == "mods/Door Fix")
assert(provenance.providers[3].size == 10i and #provenance.providers[3].hashBlake3 == 64)

local semantic = layer:semanticConflicts(vfs, { archiveHashMode = "allProviders", includeSemanticDeltas = true })
local door = semantic.entries[1]
assert(door.key == "meshes/x/ex_door.nif" and door.distinctVersions == 3 and not door.allIdentical)
assert(door.providers[1].relation == "differentFromWinner")
assert(door.providers[3].semanticDeltaToWinner.kind == "noOpEquivalent")
```
