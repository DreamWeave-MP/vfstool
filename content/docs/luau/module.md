+++
title = "@dream/vfs"
description = "Requiring the module, the conventions every function follows, the path and glob helpers, analyzePair, the run workflow, serialize, and the errors scripts see."
weight = 10

[extra]
kind = "api"
+++

```lua
local vfstool = require("@dream/vfs")
```

A frozen table: five constructor tables, the path and glob helpers, `analyzePair`, the run
workflow and, with the `serialize` feature, `serialize`. Assigning to it, or to one of its
constructor tables, raises `attempt to modify a readonly table`. The host provides the module by
composing the [extension](@/docs/luau/extension.md) into its runtime plan; a script only requires
it.

## The example install

Every example on these pages runs as written against this install, from the folder that holds
it:

{% tree() %}
Data Files/
  Extras.zip  holds Textures/tx_extra.dds and meshes/x/ex_door.nif
  Meshes/
    x/
      Ex_Door.nif
  Textures/
    Tx_Wood_01.dds
    tx_stone.dds
mods/
  Door Fix/
    meshes/
      x/
        ex_door.nif
  Wood Retexture/
    textures/
      tx_wood_01.dds
      tx_wood_02.dds
{% end %}

Relative paths are relative to the host process's working directory, as they are in Rust.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories(
    { "Data Files", "mods/Wood Retexture", "mods/Door Fix" },
    { archives = { "Extras.zip" } }
)
assert(vfs:len() == 6)
assert(vfs:getFile([[Textures\TX_WOOD_01.DDS]]):path() == "mods/Wood Retexture/textures/tx_wood_01.dds")
assert(vfs:getFile("textures/tx_extra.dds"):parentArchiveName() == "Extras.zip")
```

## Conventions

- **Paths are byte strings.** No UTF-8 is required anywhere. A key spelled with bytes that are not
  UTF-8 is a key like any other, and `file:path()` returns the exact bytes of the host path or
  archive entry name. On Windows, a host path is made from the string's bytes read as UTF-8, with
  invalid sequences replaced.
- **Keys are normalized once.** A VFS key may be written any way: `Textures\Tx_Wood_01.DDS` and
  `textures/tx_wood_01.dds` are the same key. The module normalizes it into a per-thread scratch
  buffer with [dream-path](https://dreamweave-mp.github.io/dream_path/)'s rules (`\` to `/`, ASCII
  lowercase, repeated and leading separators removed) and looks it up without allocating. A key
  that is already normalized is used as it is.
- **Host paths are not normalized.** Data directories, source paths and archive paths are used as
  given. `removeProvider`, `removeSource` and `sourceIdForPath` compare them component by
  component, case and `\` included, so `vfs:removeSource("mods/Wood Retexture")` does not match
  a source added as `mods\Wood Retexture`. `archiveEntries`, `filesFromArchive` and `remaining`
  fold both sides like `normalizeHostPath` first.
- **camelCase.** Functions, methods, table fields, and the strings that stand for Rust enums:
  `"looseDir"`, `"archive"`, `"cosmeticOnly"`, `"winnerHashChanged"`.
- **1-based indices.** `sourceIndex`, `providerIndex` and `winnerIndex` in reports, the results of
  `sourcesContaining` and `sourceIdForPath`, and the arguments of `sourceById` and
  `providerOriginalPath` all count from 1.
- **Sizes are integers.** Byte sizes (`size`, `winnerSize`) are Luau integers, so `==` compares
  them with `4i`; counts and indices are numbers.
- **Strict options.** An option table takes its own keys and no others. A misspelled or snake_case
  option is an error that lists the known options; it never falls back to a default.
- **Lists that scripts walk are views.** `keys`, `entries`, `pathsMatching`, `pathsWith`,
  `providersFor`, `providerRecordsFor`, `filesFromArchive`, `layer:keys` and
  `layer:duplicateKeys` return [sequence views](@/docs/luau/views.md), and `tree`, `findByRegex`
  and `remaining` return a [tree stream](@/docs/luau/views.md#tree). Reports are plain tables,
  described on [Report tables](@/docs/luau/reports.md).
- **No `io`.** Luau has no `io` library. Scripts read files through `VfsFile`; anything else that
  reads or writes the disk is the host's to provide.
- **Features.** `vfs:pushArchive` exists only when the crate is built with `beth-archives` (BSA
  and BA2) or `zip` (ZIP, and renamed ZIPs such as PK3); without either, archive lists are
  ignored and an archive is a plain file. `serialize` and `vfs:serializeTree` exist only with
  `serialize`.

Signatures on these pages name the module's types by their short names: `VFS`, `VfsFile`,
`Keys`, `Tree`. The [type definitions](@/docs/luau/types.md) call them `dream_vfs_VFS`,
`dream_vfs_VfsFile` and so on. Names such as `Explain` or `Source` label a table shape from
[Report tables](@/docs/luau/reports.md); the definitions spell those tables out in full.

## Constructors

| Table | Functions | Page |
|---|---|---|
| `vfstool.VFS` | `new`, `fromDirectories`, `fromDirectoriesWithConflictIndex`, `fromDirectoriesWithLayerIndex` | [VFS](@/docs/luau/vfs.md) |
| `vfstool.VfsFile` | `from` | [VfsFile and VfsProvider](@/docs/luau/files.md) |
| `vfstool.VfsProvider` | `new` | [VfsFile and VfsProvider](@/docs/luau/files.md) |
| `vfstool.LayerIndex` | `fromFileLists` | [LayerIndex, VfsLock and ConflictIndex](@/docs/luau/indexes.md) |
| `vfstool.ConflictIndex` | `fromDirectories`, `fromFileLists`, `fromLayerIndex` | [LayerIndex, VfsLock and ConflictIndex](@/docs/luau/indexes.md) |

## Paths and globs

### normalizeHostPath

{{ api_signature(value="normalizeHostPath(path: string) -> string") }}

`\` becomes `/` and ASCII `A` to `Z` become lowercase. Nothing else changes: separators are not
collapsed and a leading `/` stays. It is for comparing host paths as text, the way the command
line compares the directory you typed with a `data=` line. A VFS key is normalized with
`@dream/path`'s `normalize` instead.

```lua
local vfstool = require("@dream/vfs")

assert(vfstool.normalizeHostPath([[C:\Games\Morrowind\Data Files]]) == "c:/games/morrowind/data files")
assert(vfstool.normalizeHostPath("//Mods//Patch") == "//mods//patch")
```

### normalizeHostPathInPlace

{{ api_signature(value="normalizeHostPathInPlace(path: string) -> string") }}

The same function under the Rust name. Luau strings are immutable, so nothing happens in place;
it returns the normalized copy.

### pathGlobMatches

{{ api_signature(value="pathGlobMatches(glob: string, path: string) -> boolean") }}

Whether a VFS key matches a glob. The glob and the path are both folded like
`normalizeHostPath` first, so case and separators do not matter, and the glob must match the whole
path:

| In the glob | Matches |
|---|---|
| `*` | Any run of characters within one component |
| `?` | One character within one component |
| `**` | Any run of characters, across components |
| `**/` | Zero or more whole directories |
| Anything else | Itself: `[`, `]`, `{` and `}` have no special meaning |

```lua
local vfstool = require("@dream/vfs")

assert(vfstool.pathGlobMatches("textures/**/*.dds", [[Textures\Tx_Wood_01.DDS]]))
assert(vfstool.pathGlobMatches("**/ex_door.nif", "meshes/x/ex_door.nif"))
assert(not vfstool.pathGlobMatches("textures/*.dds", "textures/a/b.dds"))
assert(vfstool.pathGlobMatches("textures/?.dds", "textures/a.dds"))
```

### sourceGlobMatches

{{ api_signature(value="sourceGlobMatches(glob: string, sourcePath: string) -> boolean") }}

The same matching, for a source path: a data directory or an archive.

```lua
local vfstool = require("@dream/vfs")

assert(vfstool.sourceGlobMatches("**/mods/*patch*", "/home/user/MODS/MyPatch"))
```

## analyzePair

{{ api_signature(value="analyzePair(path: string, leftBytes: string, rightBytes: string) -> SemanticPair") }}

Compares two versions of one file: what kind of file it is, and how much the difference matters.
`path` only helps choose the kind; nothing is read from disk.

```lua
local vfstool = require("@dream/vfs")

local result = vfstool.analyzePair("settings.ini", "[x]\na = 1\n", "# reordered\n[x]\na = 1\n")
assert(result.assetClass == "ini")
assert(result.delta.kind == "cosmeticOnly")

result = vfstool.analyzePair("settings.ini", "[x]\na = 1\n", "[x]\na = 2\n")
assert(result.delta.kind == "behaviorChanging")
assert(result.delta.changeSummary[1] == "INI keys/values differ")
```

`assetClass` is `"ini"`, `"toml"`, `"json"`, `"luaScript"`, `"mwScriptLike"`, `"text"`,
`"binary"` or `"unknown"`. `delta.kind` is `"noOpEquivalent"`, `"cosmeticOnly"`,
`"behaviorChanging"` or `"unknown"`; only `"behaviorChanging"` has a `changeSummary`. How each
class is chosen and compared is on the Rust [semantic analysis](@/docs/api/semantic.md) page.
Without the `serialize` feature, JSON and TOML differences are `"unknown"`.

## The run workflow

Dump the VFS into a scratch directory, let a tool work on it, and collect what the tool wrote.
The command line's [`run`](@/docs/run.md) does the same around a child process.

### runSetup

{{ api_signature(value="runSetup(vfs: VFS, mergedDir: string, useHardlinks: boolean) -> (number, Snapshot)") }}

Writes every file the VFS resolves into `mergedDir`, files that live in archives included, and
returns how many it wrote and a [snapshot](@/docs/luau/indexes.md#snapshots) of their content.

{% callout(kind="danger", title="mergedDir is deleted first") %}
If `mergedDir` exists, it is removed, with everything in it, before the dump, so the tool sees
only the VFS. Pass a scratch directory, never one that holds anything you want. The command line
refuses a directory that is not empty; this function does not.
{% end %}

With `useHardlinks`, loose files are hard links to the originals: fast and free on disk, but a
tool that edits a file in place edits the original too. Pass `false` for any tool that is not
hardlink-safe. A loose source that no longer exists, or an archive entry that cannot be read, is an
error.

### runFinalize

{{ api_signature(value="runFinalize(mergedDir: string, outputDir: string, snapshot: Snapshot) -> { Copied }") }}

Copies every file in `mergedDir` that is new or whose content differs from the snapshot into
`outputDir`, at the same relative path, and returns one `{ relativePath, destinationPath }` row per
file, sorted by path. Deleted files are not reported. A destination that already is the changed
file, because the tool rewrote it in place through a hard link from `outputDir`, is left as it is.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local count, snapshot = vfstool.runSetup(vfs, "merged", false)
assert(count == 5 and snapshot:len() == 5)

-- The tool runs here. writeFile stands in for it: Luau cannot write files, so the host
-- provides whatever does.
writeFile("merged/textures/tx_stone.dds", "edited stone")
writeFile("merged/textures/tx_new.dds", "new")

local changed = vfstool.changedFiles("merged", snapshot)
assert(#changed == 2 and changed[1] == "textures/tx_new.dds" and changed[2] == "textures/tx_stone.dds")

local copied = vfstool.runFinalize("merged", "captured", snapshot)
assert(#copied == 2)
assert(copied[2].relativePath == "textures/tx_stone.dds")
assert(copied[2].destinationPath == "captured/textures/tx_stone.dds")
```

### runSetupTracked and runFinalizeTracked

{{ api_signature(value="runSetupTracked(vfs: VFS, mergedDir: string, useHardlinks: boolean) -> (number, MetadataSnapshot)") }}

{{ api_signature(value="runFinalizeTracked(mergedDir: string, outputDir: string, snapshot: MetadataSnapshot) -> { Copied }") }}

The same pair with a snapshot that also records each file's size and modification time: the one
the command line uses. Which files are copied is still decided by content; a file whose size and
time match is hashed anyway, because tools rewrite files in place.

### snapshotDirectory and snapshotDirectoryMetadata

{{ api_signature(value="snapshotDirectory(dir: string) -> Snapshot") }}

{{ api_signature(value="snapshotDirectoryMetadata(dir: string) -> MetadataSnapshot") }}

A snapshot of any directory: every file under it, hashed with BLAKE3. A directory that does not
exist is an error.

### changedFiles and changedFilesMetadata

{{ api_signature(value="changedFiles(dir: string, snapshot: Snapshot) -> { string }") }}

{{ api_signature(value="changedFilesMetadata(dir: string, snapshot: MetadataSnapshot) -> { string }") }}

The paths, relative to `dir` and sorted, of the files that are new or whose content differs from
the snapshot. Nothing is copied.

## serialize

{{ api_signature(value="serialize(value: any, format: string) -> string") }}

With the `serialize` feature: `value` written as `"json"`, `"yaml"` or `"toml"`, for a report the
host will save.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Door Fix" })
assert(vfstool.serialize(vfs:keys(), "json")
    == [=[["extras.zip","meshes/x/ex_door.nif","textures/tx_stone.dds","textures/tx_wood_01.dds"]]=])
assert(vfstool.serialize({ 1, 2, 3 }, "json") == "[1,2,3]")
assert(vfstool.serialize({ a = { b = true } }, "toml") == "[a]\nb = true\n")

local explain = vfs:explain("meshes/x/ex_door.nif")
assert(vfstool.serialize(explain, "yaml"):find("winner:", 1, true))
```

- A table whose keys are exactly `1` to `n` is an array, and `{}` is an empty array. Any other
  table is an object, with number keys written as strings. JSON objects come out with their keys
  sorted.
- A number with no fraction is written as an integer; Luau integers are integers.
- Strings, and string keys, must be UTF-8.
- `Keys` and `ProviderRecords` views are written as the arrays their `toTable()` gives. Any other
  userdata is an error: that includes `Entries` and `Providers` views, whose rows hold `VfsFile`
  handles, and the tree stream.
- TOML needs a table at the top; an array there is an error.
- Nesting is limited to 128 levels.

## Errors

Errors are raised as Luau errors, catchable with `pcall`, and say where they come from. The texts
below are exact.

A wrong argument type is reported by the binder, naming the function and the argument:

| Mistake | Error |
|---|---|
| `vfs:getFile(42)` | `dream.vfs.VFS.getFile: bad argument #1 (expected string): Lua stack index 2: expected string, got number` |
| `vfs.len(file)`, with a `VfsFile` | `invalid argument #1 to 'dream.vfs.VFS.len' (dream.vfs.VFS expected, got dream.vfs.VfsFile)` |
| `vfs.len()`, a dot instead of a colon | `missing argument #1 to 'dream.vfs.VFS.len' (dream.vfs.VFS expected)`, after the script position |
| `layer:sourceById(1.5)` | `dream.vfs.LayerIndex.sourceById: bad argument #1 (expected integer): Lua stack index 2: number 1.5 is not an exact i64` |

An array of paths names the argument and the element:

| Mistake | Error |
|---|---|
| `VFS.fromDirectories(42)` | `dream.vfs: dirs: Lua stack index 1: expected table, got number` |
| `VFS.fromDirectories({ 42 })` | `dream.vfs: dirs[1]: expected string, got number` |

An option table names the function and the field:

| Mistake | Error |
|---|---|
| `VFS.fromDirectories(dirs, { archive = {} })` | `VFS.fromDirectories: unknown option 'archive'; known options are archives` |
| `VFS.fromDirectories(dirs, { archives = "a.bsa" })` | `VFS.fromDirectories.archives: expected table, got string` |
| `VFS.fromDirectories(dirs, { archives = { 1 } })` | `VFS.fromDirectories.archives[1]: expected string, got number` |
| `vfs:collapseInto(dest, { allow_copying = true })` | `vfs:collapseInto: unknown option 'allow_copying'; known options are allowCopying, extractArchives, useSymlinks` |
| `vfs:materializationPlan(dest, { extractArchives = "yes" })` | `vfs:materializationPlan.extractArchives: expected boolean, got string` |
| `LayerIndex.fromFileLists({ { source = { path = "x" }, files = {} } })` | `LayerIndex.fromFileLists[1].source: missing required option 'kind'` |
| `VfsProvider.new({ path = "x", kind = "zip" }, file)` | `VfsProvider.new.kind: dream.vfs: unknown source kind 'zip' (expected 'looseDir' or 'archive')` |
| `layer:semanticConflicts(vfs, { archiveHashMode = "sometimes" })` | `layer:semanticConflicts.archiveHashMode: expected 'disabled', 'winnerOnly', or 'allProviders', got 'sometimes'` |

Everything else starts with `dream.vfs:`:

| Cause | Error |
|---|---|
| An index of 0 or less | `dream.vfs: source id must be a 1-based index, got 0`; `source index` for `providerOriginalPath` |
| A bad regular expression | `dream.vfs: invalid regex: ` and the regex crate's message, which spans several lines |
| A file that cannot be read | `dream.vfs: No such file or directory (os error 2)`, or whatever the system says |
| A directory that cannot be walked | `dream.vfs: IO error for operation on nope: No such file or directory (os error 2)` |
| A negative `readInto` offset | `dream.vfs: negative buffer offset` |
| An unknown serialization format | `dream.vfs: unknown serialization format 'xml' (expected json, yaml, or toml)` |
| A value `serialize` cannot write | `dream.vfs: serialize: ` and the reason: `strings must be valid UTF-8`, `cannot serialize a function`, `a boolean cannot be an object key`, `a non-finite number has no representation`, `value nests deeper than 128 levels`, `userdata other than a sequence view cannot be serialized` |
| A TOML document that is not a table | `dream.vfs: unsupported array type` |

A `readInto` offset past the end of the buffer raises `buffer access out of bounds`, the text of
Luau's own buffer functions.
