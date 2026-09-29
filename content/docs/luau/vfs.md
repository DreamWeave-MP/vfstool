+++
title = "VFS"
description = "The VFS userdata: its four constructors, lookups, provider reports, every change a script can make, and writing it out as a directory."
weight = 20

[extra]
kind = "api"
+++

A `VFS` is the virtual file system OpenMW builds from its `data=` directories and
`fallback-archive=` lines: for every key, the stack of files that provide it, lowest priority
first, and the winner on top. The Rust type is described on the [Rust API](@/docs/api/vfs.md)
pages; this page is what a script sees. [How files resolve](@/docs/resolution.md) explains the
priority rules.

The examples run against the [example install](@/docs/luau/module.md#the-example-install).

## Constructors

### VFS.new

{{ api_signature(value="VFS.new() -> VFS") }}

An empty VFS, for building one up with `pushDirectory`, `pushArchive` and `pushProvider`.

### VFS.fromDirectories

{{ api_signature(value="VFS.fromDirectories(dirs: { string }, options: { archives: { string }? }?) -> VFS") }}

Builds the VFS OpenMW would. `dirs` are data directories, lowest priority first: a file in a later
directory wins over the same key in an earlier one. Each directory is walked in full, following
symbolic links.

`archives` names archives as `openmw.cfg`'s `fallback-archive=` lines do, by file name. Each is
looked up as a key among the loose files of all the directories and opened; its entries go below
every loose file, and a later archive in the list wins over an earlier one. BSA and BA2 files are
recognized by their contents, ZIP archives by their extension. The archive file itself stays a
loose file in the VFS, as it does in OpenMW.

Building never fails. A directory that does not exist adds nothing, an archive that cannot be
found or opened is left out, and so is a file whose key would be unsafe or would put a file where
another key needs a directory. What is left can always be written out.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories(
    { "Data Files", "mods/Wood Retexture", "mods/Door Fix", "mods/Missing" },
    { archives = { "Extras.zip", "Missing.bsa" } }
)
assert(vfs:len() == 6)
assert(vfs:getFile("meshes/x/ex_door.nif"):path() == "mods/Door Fix/meshes/x/ex_door.nif")
assert(vfs:getFile("textures/tx_extra.dds"):isArchive())
assert(vfs:contains("extras.zip"))
```

### VFS.fromDirectoriesWithConflictIndex

{{ api_signature(value="VFS.fromDirectoriesWithConflictIndex(dirs: { string }, options: { archives: { string }? }?) -> (VFS, ConflictIndex)") }}

The same VFS and its [`ConflictIndex`](@/docs/luau/indexes.md#conflictindex), from one walk of
the directories.

### VFS.fromDirectoriesWithLayerIndex

{{ api_signature(value="VFS.fromDirectoriesWithLayerIndex(dirs: { string }, options: { archives: { string }? }?) -> (VFS, LayerIndex)") }}

The same VFS and its [`LayerIndex`](@/docs/luau/indexes.md#layerindex), built up front.

## Looking up

### len and isEmpty

{{ api_signature(value="vfs:len() -> number") }}

{{ api_signature(value="vfs:isEmpty() -> boolean") }}

How many keys resolve, counted in constant time, and whether none do.

### getFile and contains

{{ api_signature(value="vfs:getFile(path: string) -> VfsFile?") }}

{{ api_signature(value="vfs:contains(path: string) -> boolean") }}

The winning [file](@/docs/luau/files.md) for a key in any spelling, or `nil`; and whether there is
one.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local wood = vfs:getFile([[Textures\Tx_Wood_01.DDS]])
assert(wood:readAll() == "new wood")
assert(vfs:contains("TEXTURES/TX_STONE.DDS"))
assert(vfs:getFile("textures/missing.dds") == nil)
```

### keys and entries

{{ api_signature(value="vfs:keys() -> Keys") }}

{{ api_signature(value="vfs:entries() -> Entries") }}

Every key, and every `{ key, file }` pair, sorted by key, as [sequence
views](@/docs/luau/views.md). `keys` sorts once and hands later calls the same list until the VFS
changes.

### pathsMatching

{{ api_signature(value="vfs:pathsMatching(substring: string) -> Entries") }}

The entries whose key contains `substring` anywhere, sorted by key. The substring is folded like
a host path, `\` to `/` and ASCII to lowercase, and compared with the key's bytes.

### pathsWith

{{ api_signature(value="vfs:pathsWith(prefix: string) -> Entries") }}

The entries at or under a directory, sorted by key. The prefix is normalized like any key and
matched on whole components, so `textures` finds `textures/tx_stone.dds` but not
`textures2/a.dds`, and `textures`, `textures/` and `Textures\` are the same prefix.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
assert(#vfs:pathsMatching("WOOD") == 2)
assert(#vfs:pathsWith([[Textures\]]) == 3)
assert(#vfs:pathsWith("tex") == 0)
for _, entry in vfs:pathsWith("textures") do
    assert(entry.key:sub(1, 9) == "textures/" and entry.file:isLoose())
end
```

### findByRegex

{{ api_signature(value="vfs:findByRegex(pattern: string, relative: boolean?) -> Tree") }}

The files whose key matches a regular expression, as a [tree](@/docs/luau/views.md#tree). The
pattern is case-insensitive and unanchored, and is matched against the key with `/` separators.
`relative` defaults to `true`; it is described under `tree`. A pattern that does not compile
raises `dream.vfs: invalid regex: ...`.

### remaining

{{ api_signature(value="vfs:remaining(filterPath: string, replacementsOnly: boolean, allDirs: { string }, relative: boolean?) -> Tree") }}

What is left of one data directory. With `replacementsOnly` false: the files the VFS still serves
from `filterPath`. With it true: the keys `filterPath` has that the VFS serves from somewhere
else. `allDirs` is the full list of data directories; `filterPath` is found in it by comparing both
as folded host paths, and its own loose files are read again from disk.

```lua
local vfstool = require("@dream/vfs")

local dirs = { "Data Files", "mods/Wood Retexture", "mods/Door Fix" }
local vfs = vfstool.VFS.fromDirectories(dirs)

local kept = {}
for _, row in vfs:remaining("Data Files", false, dirs) do
    table.insert(kept, row.file:path())
end
assert(#kept == 2 and kept[1] == "Data Files/Extras.zip" and kept[2] == "Data Files/Textures/tx_stone.dds")

local replaced = vfs:remaining("Data Files", true, dirs)
assert(replaced:count() == 2)
```

### tree

{{ api_signature(value="vfs:tree(relative: boolean?) -> Tree") }}

Every file as a sorted directory [tree](@/docs/luau/views.md#tree). With `relative`, the default,
the root is `Data Files` and directories are the keys' own; an archive's files sit under a
directory named after the archive. With `relative` set to `false`, files sit under their host
paths, below a root named `/`.

### display

{{ api_signature(value="vfs:display(relative: boolean?) -> string") }}

The tree as text, for a quick look. Each directory that holds files directly is one
`├── name/` line with its last component only, followed by a `│   ├── file` line per file;
directories that hold only directories get no line. Nested and same-named directories are
therefore hard to tell apart. `tree` has the structure. `tostring(vfs)` is
`dream.vfs.VFS(6 files)`, not the tree.

### serializeTree

{{ api_signature(value="vfs:serializeTree(relative: boolean?, format: string) -> string") }}

With the `serialize` feature: the tree as `"json"`, `"yaml"` or `"toml"`. Each directory is an
object whose `"."` member lists its files by name, and whose other members are its
subdirectories. Pass `nil` for `relative` to keep the default.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "mods/Wood Retexture", "mods/Door Fix" })
assert(vfs:serializeTree(nil, "json")
    == [[{"Data Files":{"meshes":{"x":{".":["ex_door.nif"]}},"textures":{".":["tx_wood_01.dds","tx_wood_02.dds"]}}}]])
```

## Providers

### providersFor

{{ api_signature(value="vfs:providersFor(key: string) -> Providers?") }}

Every provider of a key, lowest priority first, as a view of `{ source, file }` rows; the last is
the winner. `nil` when nothing provides the key.

### providerRecordsFor

{{ api_signature(value="vfs:providerRecordsFor(path: string) -> ProviderRecords") }}

The same providers as report rows: source index, source, key, the path the source spelled it with,
and the resolved path. Empty when nothing provides the key. The row is described under
[provider records](@/docs/luau/reports.md#provider-records).

### explain

{{ api_signature(value="vfs:explain(path: string) -> Explain?") }}

Why a key resolves the way it does: the winning provider and every provider it overrides, or
`nil` for an unknown key. The command line's `explain` prints the same report.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories(
    { "Data Files", "mods/Door Fix" },
    { archives = { "Extras.zip" } }
)
local report = vfs:explain([[Meshes\X\Ex_Door.NIF]])
assert(report.key == "meshes/x/ex_door.nif")
assert(report.winner.source.path == "mods/Door Fix" and report.winner.sourceIndex == 3)
assert(#report.overridden == 2)
assert(report.overridden[1].source.kind == "archive")
assert(report.overridden[1].resolvedPath == "Data Files/Extras.zip::meshes/x/ex_door.nif")
assert(report.overridden[2].originalPath == "Meshes/x/Ex_Door.nif")
```

A `sourceIndex` in these reports counts the VFS's own sources, in the order they were added:
archives from the constructor first, then the directories, then whatever was pushed since. A
source keeps its number after it stops providing anything, so after changes the numbers can differ
from a `LayerIndex`'s, which lists only the sources that still provide a file.

### duplicates

{{ api_signature(value="vfs:duplicates(pattern: string?) -> Duplicates") }}

Every key with more than one provider, sorted by key, each with all its providers and the index of
the winner. `pattern` keeps only keys that match it, as a case-insensitive regular expression.

### archives

{{ api_signature(value="vfs:archives() -> { ArchiveInfo }") }}

Every archive source, sorted by path: how many entries it provides and how many of those win.

### archiveEntries and filesFromArchive

{{ api_signature(value="vfs:archiveEntries(archive: string) -> { ArchiveEntry }") }}

{{ api_signature(value="vfs:filesFromArchive(archive: string) -> Keys") }}

What one archive provides, sorted by key: each entry with its spelling inside the archive and
whether it wins, or just the keys. `archive` is the archive's path as `archives()` reports it,
compared as a folded host path; a bare file name matches nothing.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories(
    { "Data Files", "mods/Door Fix" },
    { archives = { "Extras.zip" } }
)
local archive = vfs:archives()[1]
assert(archive.path == "Data Files/Extras.zip")
assert(archive.entryCount == 2 and archive.winningEntryCount == 1)

local entries = vfs:archiveEntries([[data files\extras.zip]])
assert(entries[1].key == "meshes/x/ex_door.nif" and not entries[1].wins)
assert(entries[2].originalPath == "Textures/tx_extra.dds" and entries[2].wins)
assert(vfs:filesFromArchive(archive.path)[2] == "textures/tx_extra.dds")
assert(#vfs:archiveEntries("Extras.zip") == 0)
```

### sourceContributions

{{ api_signature(value="vfs:sourceContributions() -> Contributions") }}

Per source: how many files it provides, wins, overrides, loses, and holds alone. The fields are
under [contributions](@/docs/luau/reports.md#contributions).

### layerIndex

{{ api_signature(value="vfs:layerIndex() -> LayerIndex") }}

A copy of the VFS's [`LayerIndex`](@/docs/luau/indexes.md#layerindex). The VFS builds it on the
first call, unless it came from `fromDirectoriesWithLayerIndex`, and keeps it current through
later changes; the copy a script holds does not follow them.

### diffDirectory

{{ api_signature(value="vfs:diffDirectory(dir: string) -> DirectoryDiff") }}

What adding a directory on top would do, without adding it: the keys where it would replace the
current winner, with both files, and the keys it would add, in no particular order. The directory
is walked like a data directory and nothing is changed; one that cannot be walked gives two empty
lists and a warning on standard error.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files" })
local diff = vfs:diffDirectory("mods/Wood Retexture")
assert(#diff.conflicts == 1 and diff.conflicts[1].key == "textures/tx_wood_01.dds")
assert(diff.conflicts[1].current:path() == "Data Files/Textures/Tx_Wood_01.dds")
assert(#diff.additions == 1 and diff.additions[1].key == "textures/tx_wood_02.dds")
```

## Changing it

Some methods replace the winner and throw away the stack under it; others add or remove single
providers and leave the rest in place. The names say which: *winner* and *resolved* work on the
resolved key, *provider* and *source* on the stack. Every change is visible at once to lookups,
reports, and the VFS's `LayerIndex`. Views and reports a script already holds keep what they had.

Keys that are unsafe (empty, absolute, with a drive letter, a `.` or `..` component, or a NUL
byte), or that would put a file where another key needs a directory (`textures` when
`textures/tx_stone.dds` exists, or the other way round), are refused: the method returns `nil` or
`false` and changes nothing.

### setWinnerFile and setWinnerLooseFile

{{ api_signature(value="vfs:setWinnerFile(key: string, file: VfsFile) -> VfsFile?") }}

{{ api_signature(value="vfs:setWinnerLooseFile(key: string, physicalPath: string) -> VfsFile?") }}

Replace the whole provider stack of `key` with one file, and return the previous winner, or `nil`
when there was none. The new provider's source is the file's archive, or the directory that holds
a loose file. `setWinnerLooseFile` is `setWinnerFile(key, VfsFile.from(physicalPath))`.

### pushDirectory

{{ api_signature(value="vfs:pushDirectory(path: string)") }}

Walks a directory like a data directory and adds every file as a provider on top of what is
there, as one new source. A directory that cannot be walked raises an I/O error.

### pushArchive

{{ api_signature(value="vfs:pushArchive(path: string) -> boolean") }}

With `beth-archives` or `zip`: opens the archive at a host path and adds every entry as one new
source on top, above every loose file. That is manual stacking, not OpenMW's rule; the
constructors put archives below loose files. `false` when the archive cannot be opened.

### pushProvider

{{ api_signature(value="vfs:pushProvider(key: string, provider: VfsProvider) -> boolean") }}

Adds one [provider](@/docs/luau/files.md#vfsprovider) on top of `key`'s stack, as a new source of
its own. `false` for a refused key.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local file = vfstool.VfsFile.from("mods/Door Fix/meshes/x/ex_door.nif")
local provider = vfstool.VfsProvider.new({ path = "mods/Door Fix", kind = "looseDir" }, file)

assert(vfs:pushProvider([[Meshes\X\Ex_Door.NIF]], provider))
assert(#vfs:providersFor("meshes/x/ex_door.nif") == 2)
assert(not vfs:pushProvider("../outside.nif", provider))

local removed = vfs:removeWinner("meshes/x/ex_door.nif")
assert(removed:source().path == "mods/Door Fix")
assert(vfs:getFile("meshes/x/ex_door.nif"):path() == "Data Files/Meshes/x/Ex_Door.nif")
```

### removeWinner

{{ api_signature(value="vfs:removeWinner(key: string) -> VfsProvider?") }}

Removes only the winner, so the provider under it wins, and returns it. `nil` for an unknown key.

### removeResolvedFile

{{ api_signature(value="vfs:removeResolvedFile(key: string) -> VfsFile?") }}

Removes the key with its whole stack and returns the file that was winning.

### removeProvider

{{ api_signature(value="vfs:removeProvider(key: string, sourcePath: string) -> { Provider }") }}

Removes every provider of `key` whose source path is `sourcePath`, and returns them. Paths are
compared component by component, with case and `\` left as they are. The next provider down wins.

### removeSource

{{ api_signature(value="vfs:removeSource(sourcePath: string) -> { RemovedProvider }") }}

Removes every provider from one source, under every key, revealing what each covered. Returns
`{ key, provider }` rows in no particular order.

### removeProviderPrefix and removeResolvedPrefix

{{ api_signature(value="vfs:removeProviderPrefix(prefix: string) -> { RemovedProvider }") }}

{{ api_signature(value="vfs:removeResolvedPrefix(prefix: string) -> { RemovedFile }") }}

Remove every key at or under a directory, matched on whole components like `pathsWith`, with all
their providers. The first returns every provider removed, as `{ key, provider }` rows; the second
returns the files that were winning, as `{ key, file }` rows. Both in no particular order.

### removeResolvedMatchingGlob

{{ api_signature(value="vfs:removeResolvedMatchingGlob(glob: string) -> { RemovedFile }") }}

Removes every key that matches a [glob](@/docs/luau/module.md#pathglobmatches), with its stack, and
returns `{ key, file }` rows for the files that were winning.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
assert(#vfs:removeSource("mods/Wood Retexture") == 2)
assert(vfs:getFile("textures/tx_wood_01.dds"):path() == "Data Files/Textures/Tx_Wood_01.dds")
assert(not vfs:contains("textures/tx_wood_02.dds"))

assert(#vfs:removeResolvedMatchingGlob("**/*.nif") == 1)
assert(#vfs:removeResolvedPrefix("textures") == 2)
assert(vfs:len() == 1)
```

## Writing it out

### dumpToDirectory

{{ api_signature(value="vfs:dumpToDirectory(dir: string, useHardlinks: boolean) -> number") }}

Writes every file the VFS resolves into `dir`, at its key, and returns how many it wrote. Files
from archives are extracted. Loose files are copied, or hard-linked with `useHardlinks`, copied
after all when the link would cross devices. Existing files in the way are replaced, except the
loose file itself when `dir` is the directory it comes from, which stays where it is. A loose
source that has disappeared, or an archive entry that cannot be read, is skipped with a line on
standard error. The [run workflow](@/docs/luau/module.md#the-run-workflow) uses this.

### collapseInto

{{ api_signature(value="vfs:collapseInto(dest: string, options: { allowCopying: boolean?, extractArchives: boolean?, useSymlinks: boolean? }?)") }}

Writes the VFS into `dest` as one directory, like the command line's `collapse`. All three options
default to `false`:

| Option | Effect |
|---|---|
| `useSymlinks` | Symbolic links to loose files instead of hard links |
| `allowCopying` | Copy a loose file when the link fails, for example across devices |
| `extractArchives` | Extract files that live in archives, and leave the archive files themselves out, each with a line on standard error |

Without `extractArchives`, files from archives are left out, each with a line on standard error.
A loose source that no longer exists, or a link that fails without `allowCopying`, raises an error.
A loose file whose destination already is that file, because `dest` is the data directory it
comes from, is left where it is.

### materializationPlan

{{ api_signature(value="vfs:materializationPlan(dest: string, options: { allowCopying: boolean?, extractArchives: boolean?, useSymlinks: boolean? }?) -> Plan") }}

What `collapseInto` with the same options would do, one action per key in key order, and the
problems it can see coming, without writing anything. The rows are under [materialization
plan](@/docs/luau/reports.md#materialization-plan).

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories(
    { "Data Files", "mods/Door Fix" },
    { archives = { "Extras.zip" } }
)
local plan = vfs:materializationPlan("merged", { extractArchives = true })
assert(#plan.issues == 0)
assert(plan.actions[1].kind == "skipArchiveFile" and plan.actions[1].key == "extras.zip")
assert(plan.actions[2].kind == "hardlink" and plan.actions[2].dest == "merged/meshes/x/ex_door.nif")
assert(plan.actions[3].kind == "extractArchive" and plan.actions[3].archive == "Data Files/Extras.zip")

vfs:collapseInto("merged", { extractArchives = true, allowCopying = true })
local merged = vfstool.VFS.fromDirectories({ "merged" })
assert(merged:len() == 4 and not merged:contains("extras.zip"))
assert(merged:getFile("textures/tx_extra.dds"):readAll() == "extra")
```

### extractFile

{{ api_signature(value="vfs:extractFile(vfsPath: string, destDir: string) -> string?") }}

Writes the winner for one key into `destDir`, under its own file name only, and returns the path
written: `nil` when the key is unknown. `destDir` is created if needed, and a file already there is
replaced.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files" }, { archives = { "Extras.zip" } })
assert(vfs:extractFile([[Textures\TX_EXTRA.DDS]], "out") == "out/tx_extra.dds")
assert(vfs:extractFile("textures/missing.dds", "out") == nil)
```
