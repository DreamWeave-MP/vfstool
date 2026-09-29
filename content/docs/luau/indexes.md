+++
title = "LayerIndex, VfsLock and ConflictIndex"
description = "The provider-occurrence index with provenance, locks, drift and semantic conflicts; the lock manifest; the source-versus-source conflict index; and the run workflow's snapshots."
weight = 40

[extra]
kind = "api"
+++

A `VFS` answers what wins. These answer why, and what changed: the `LayerIndex` records every
provider of every key in load order, a `VfsLock` pins down the winners at one moment, and a
`ConflictIndex` reduces it all to which source overrides which. The Rust side is on
[LayerIndex](@/docs/api/layer-index.md), [Locks and drift](@/docs/api/lock-drift.md) and
[ConflictIndex and reports](@/docs/api/conflicts.md).

The examples run against the [example install](@/docs/luau/module.md#the-example-install).

## LayerIndex

Every provider of every key, lowest priority first, and the sources they come from. Get one from
`VFS.fromDirectoriesWithLayerIndex`, from `vfs:layerIndex()`, or build one from lists of paths.
Its sources are the VFS's sources that provide at least one file, archives first, numbered from 1.

### LayerIndex.fromFileLists

{{ api_signature(value="LayerIndex.fromFileLists(sources: { { source: { path: string, kind: string }, files: { string } } }) -> LayerIndex") }}

An index over sources you describe: each row a source, lowest priority first, and the files it
provides, as paths in the source's own spelling. Nothing is read from disk. Rows are strict option
tables with `source` and `files` required.

```lua
local vfstool = require("@dream/vfs")

local layer = vfstool.LayerIndex.fromFileLists({
    { source = { path = "base", kind = "looseDir" }, files = { "Textures/A.dds", "b.txt" } },
    { source = { path = "patch.bsa", kind = "archive" }, files = { "textures/a.dds" } },
})
assert(#layer:keys() == 2 and layer:duplicateKeys()[1] == "textures/a.dds")
local chain = layer:providerChain("textures/a.dds")
assert(chain[1].source.path == "base" and chain[1].originalPath == "Textures/A.dds")
assert(chain[2].source.kind == "archive")
```

### keys and duplicateKeys

{{ api_signature(value="layer:keys() -> Keys") }}

{{ api_signature(value="layer:duplicateKeys() -> Keys") }}

Every key, and every key with more than one provider, sorted, as [views](@/docs/luau/views.md).

### sources

{{ api_signature(value="layer:sources() -> { Source }") }}

The sources in load order, as `{ path, kind }` tables.

### sourceIdForPath and sourceById

{{ api_signature(value="layer:sourceIdForPath(path: string) -> number?") }}

{{ api_signature(value="layer:sourceById(id: number) -> Source?") }}

A source's 1-based position from its path, compared component by component with case and `\`
as they are, and a source from its position; `nil` when there is none. An id of 0 or less is an
error, not `nil`.

### sourcesContaining

{{ api_signature(value="layer:sourcesContaining(path: string) -> { number }") }}

The positions of the sources that provide a key, lowest priority first. The key may be spelled
any way.

### providerOriginalPath

{{ api_signature(value="layer:providerOriginalPath(sourceIndex: number, path: string) -> string?") }}

How the source at `sourceIndex` spells a key, such as `Meshes/x/Ex_Door.nif` for
`meshes/x/ex_door.nif`; `nil` when that source does not provide it.

### providerChain

{{ api_signature(value="layer:providerChain(path: string) -> { LayerProvider }") }}

Every provider of a key, lowest priority first, with its source, its position among the key's
providers, and its original spelling. The rows are under [layer
providers](@/docs/luau/reports.md#layer-providers).

```lua
local vfstool = require("@dream/vfs")

local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex(
    { "Data Files", "mods/Wood Retexture", "mods/Door Fix" },
    { archives = { "Extras.zip" } }
)
assert(layer:sources()[1].path == "Data Files/Extras.zip" and layer:sources()[1].kind == "archive")
assert(layer:sourceIdForPath("mods/Door Fix") == 4)
assert(layer:sourceById(2).path == "Data Files")

local containing = layer:sourcesContaining([[Meshes\X\Ex_Door.NIF]])
assert(#containing == 3 and containing[3] == 4)
assert(layer:providerOriginalPath(2, "meshes/x/ex_door.nif") == "Meshes/x/Ex_Door.nif")

local chain = layer:providerChain("meshes/x/ex_door.nif")
assert(chain[3].sourceIndex == 4 and chain[3].providerIndex == 3)
```

### sourceContributions

{{ api_signature(value="layer:sourceContributions() -> Contributions") }}

Per source: files provided, won, overriding, overridden, unique and duplicated, loose and
archived. The same report as `vfs:sourceContributions()`; the fields are under
[contributions](@/docs/luau/reports.md#contributions).

### provenance

{{ api_signature(value="layer:provenance(vfs: VFS, path: string, withHashes: boolean) -> Provenance?") }}

The whole chain behind one key in `vfs`: the winning source and every provider, with its resolved
path and, with `withHashes`, the BLAKE3 hash and size of its content. `nil` for an unknown key.
Hashing reads every provider, archived ones included.

### lockManifest

{{ api_signature(value="layer:lockManifest(vfs: VFS) -> VfsLock") }}

A [lock](#vfslock) of `vfs`'s current winners: for every key, the winning source, its kind, the
hash and size of the winning content, and how many providers the key has. Every winner is read
and hashed; one that has disappeared from disk has no hash or size.

`layer` and `vfs` must describe the same load order, as they do when both come from one
constructor or the layer from `vfs:layerIndex()`. A key whose winner the two disagree on is left
out of the lock, and has no provenance.

### diffAgainstLock

{{ api_signature(value="layer:diffAgainstLock(vfs: VFS, lock: VfsLock) -> Drift") }}

How `vfs` has drifted from a lock: one row per key and kind of change, `"added"`, `"removed"`,
`"winnerSourceChanged"`, `"winnerHashChanged"` or `"providerCountChanged"`, and a count per kind.
A key can drift in several ways at once. No drift is an empty `entries`.

```lua
local vfstool = require("@dream/vfs")

local archives = { archives = { "Extras.zip" } }
local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex(
    { "Data Files", "mods/Wood Retexture", "mods/Door Fix" }, archives)
local lock = layer:lockManifest(vfs)
assert(#layer:diffAgainstLock(vfs, lock).entries == 0)

-- The same install without the retexture.
local later, laterLayer = vfstool.VFS.fromDirectoriesWithLayerIndex(
    { "Data Files", "mods/Door Fix" }, archives)
local drift = laterLayer:diffAgainstLock(later, lock)
assert(#drift.entries == 4)
assert(drift.counts.removed == 1 and drift.counts.winnerSourceChanged == 1)
assert(drift.counts.winnerHashChanged == 1 and drift.counts.providerCountChanged == 1)
```

### semanticConflicts

{{ api_signature(value="layer:semanticConflicts(vfs: VFS, options: { archiveHashMode: string?, includeSemanticDeltas: boolean? }?) -> SemanticConflicts") }}

For every key with more than one provider: whether the providers' contents are the same as the
winner's, how many distinct versions there are, and, when asked, how each differs from the winner.
Loose providers are always hashed. `archiveHashMode` decides archived ones:

| `archiveHashMode` | Archived providers |
|---|---|
| `"disabled"` | Not read; their relation is `"unknown"` |
| `"winnerOnly"`, the default | Read only when they win |
| `"allProviders"` | All read |

`includeSemanticDeltas`, off by default, compares each provider with the winner the way
[`analyzePair`](@/docs/luau/module.md#analyzepair) does. An archive entry that cannot be read is an
error, not an `"unknown"`. The rows are under [semantic
conflicts](@/docs/luau/reports.md#semantic-conflicts).

`tostring(layer)` is `dream.vfs.LayerIndex(<n> sources)`.

## VfsLock

A deterministic manifest of the winners at one moment, from `layer:lockManifest`. Scripts cannot
build one from a table or a file: comparing with a lock saved earlier means keeping the handle, or
letting the host load it in Rust and push it. The command line's `lock` and `drift` save and read
the same manifest as YAML, JSON or TOML.

### schemaVersion

{{ api_signature(value="lock:schemaVersion() -> number") }}

The manifest's format version: 1.

### entries

{{ api_signature(value="lock:entries() -> { LockEntry }") }}

One row per key, sorted by key. The fields are under [lock
entries](@/docs/luau/reports.md#lock-entries).

### toTable

{{ api_signature(value="lock:toTable() -> { schemaVersion: number, entries: { LockEntry } }") }}

The whole manifest as a plain table, ready for `serialize`.

```lua
local vfstool = require("@dream/vfs")

local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex({ "Data Files", "mods/Door Fix" })
local lock = layer:lockManifest(vfs)
assert(lock:schemaVersion() == 1)
local door = lock:entries()[2]
assert(door.key == "meshes/x/ex_door.nif" and door.winnerSource == "mods/Door Fix")
assert(door.winnerSize == 10i and door.providerCount == 2 and #door.winnerHashBlake3 == 64)
assert(vfstool.serialize(lock:toTable(), "json"):find([["schemaVersion":1]], 1, true))
```

## ConflictIndex

Which source overrides which: for every source, the keys it takes from lower sources and the keys
higher sources take from it. It is derived from the same load order as a `LayerIndex`, but counts a
source only once per key, so a mod never conflicts with itself.

### Constructors

{{ api_signature(value="ConflictIndex.fromDirectories(dirs: { string }) -> ConflictIndex") }}

{{ api_signature(value="ConflictIndex.fromFileLists(sources: { { source: string, files: { string } } }) -> ConflictIndex") }}

{{ api_signature(value="ConflictIndex.fromLayerIndex(layer: LayerIndex) -> ConflictIndex") }}

From data directories, walked like `VFS.fromDirectories` but without archives; from lists of
paths per source, lowest priority first, with nothing read from disk; or from a `LayerIndex`,
archives included. `VFS.fromDirectoriesWithConflictIndex` gives one with archives in the same walk
as its VFS.

### sources

{{ api_signature(value="conflicts:sources() -> { string }") }}

The source paths in load order.

### sourcesContaining

{{ api_signature(value="conflicts:sourcesContaining(path: string) -> { number }") }}

The 1-based positions of the sources that provide a key, when more than one does. A key only one
source provides is not a conflict and gives an empty table.

### conflictsReport

{{ api_signature(value="conflicts:conflictsReport(relative: boolean?) -> ConflictsReport") }}

Every source with the keys it overrides and the keys it is overridden on. With `relative`, the
default, paths are keys; with `false`, each is the source path joined with the file's own
spelling inside it, and an archive's are written `archive::key`.

### shadowedReport

{{ api_signature(value="conflicts:shadowedReport(relative: boolean?, listFiles: boolean?) -> ShadowedReport") }}

The sources that provide at least one file and lose every one of them. `relative` works as for
`conflictsReport`. `listFiles` defaults to `true` here, unlike the command line, and lists each
shadowed source's files; `false` leaves the lists empty.

### diffReport

{{ api_signature(value="conflicts:diffReport(sourceA: string, sourceB: string) -> DiffReport") }}

The keys two sources share and the keys only one of them has, sorted, as the index holds them:
archives and names from `fromFileLists` compare like data directories. A name matches a source
spelled like it, or with ASCII case and `\` folded when none is; one that matches no source is
read from disk as a data directory. `higherPriority` is whichever comes later in the index, or
`sourceB` when either is not in it.

```lua
local vfstool = require("@dream/vfs")

local dirs = { "Data Files", "mods/Wood Retexture", "mods/Door Fix" }
local vfs, conflicts = vfstool.VFS.fromDirectoriesWithConflictIndex(dirs)
assert(#conflicts:sources() == 3 and #conflicts:sourcesContaining("textures/tx_stone.dds") == 0)

local report = conflicts:conflictsReport()
local base = report.sources[1]
assert(base.path == "Data Files" and #base.overriddenBy == 2 and #base.overrides == 0)

local diff = conflicts:diffReport("Data Files", "mods/Wood Retexture")
assert(diff.higherPriority == "mods/Wood Retexture")
assert(diff.shared[1] == "textures/tx_wood_01.dds" and diff.onlyInB[1] == "textures/tx_wood_02.dds")

local fromLists = vfstool.ConflictIndex.fromFileLists({
    { source = "base", files = { "a.txt" } },
    { source = "patch", files = { "A.TXT" } },
})
assert(fromLists:shadowedReport().sources[1].path == "base")
```

`tostring(conflicts)` is `dream.vfs.ConflictIndex(<n> sources)`.

## Snapshots

`Snapshot` and `MetadataSnapshot` are the baselines of the [run
workflow](@/docs/luau/module.md#the-run-workflow): a map from each file's path, relative to the
directory, to its content hash, and for the metadata kind its size and modification time too.
Scripts cannot look inside; they pass them back to `runFinalize`, `changedFiles` and their
`Tracked` and `Metadata` counterparts.

{{ api_signature(value="snapshot:len() -> number") }}

How many files the snapshot holds. It is the one method both kinds have.
