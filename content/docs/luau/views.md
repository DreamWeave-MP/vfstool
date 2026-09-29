+++
title = "Sequence views and trees"
description = "Keys, Entries, Providers and ProviderRecords, the sequence views over native lists, and the Tree stream: length, indexing, iteration, toTable and count, and what each costs."
weight = 50

[extra]
kind = "api"
+++

A VFS can hold hundreds of thousands of keys. The methods that return lists of them do not copy
the list into a Luau table; they return a view over the list as it sits in Rust, and push an item
onto the Luau stack only when the script reaches for it. The examples run against the [example
install](@/docs/luau/module.md#the-example-install).

## Sequence views

| View | Items | Returned by |
|---|---|---|
| `Keys` | `string` | `vfs:keys()`, `vfs:filesFromArchive()`, `layer:keys()`, `layer:duplicateKeys()` |
| `Entries` | `{ key: string, file: VfsFile }` | `vfs:entries()`, `vfs:pathsMatching()`, `vfs:pathsWith()` |
| `Providers` | `{ source: Source, file: VfsFile }` | `vfs:providersFor()` |
| `ProviderRecords` | a [provider record](@/docs/luau/reports.md#provider-records) | `vfs:providerRecordsFor()` |

Every view has the same four operations:

| Operation | Result |
|---|---|
| `#view` | The number of items |
| `view[i]` | Item `i`, counting from 1; `nil` for 0, past the end, for a number with a fraction, and for anything that is not a number |
| `for index, item in view do` | Each item with its index, in order |
| `view:toTable()` | A plain table of all the items, the shape the 0.10 binding returned |

A view is userdata, not a table: `ipairs`, `pairs` and the `table` library refuse it, with
`invalid argument #1 to 'ipairs' (table expected, got dream.vfs.Keys)`. Call `toTable()` first
for those.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local keys = vfs:keys()
assert(#keys == 5 and keys[1] == "extras.zip" and keys[5] == "textures/tx_wood_02.dds")
assert(keys[0] == nil and keys[6] == nil and keys[1.5] == nil)

local count = 0
for index, key in keys do
    assert(keys[index] == key)
    count += 1
end
assert(count == 5)

assert(not pcall(ipairs, keys))
local plain = keys:toTable()
table.sort(plain, function(a, b) return a > b end)
assert(plain[1] == "textures/tx_wood_02.dds")

local entry = vfs:entries()[4]
assert(entry.key == "textures/tx_wood_01.dds" and entry.file:readAll() == "new wood")
```

A view is a snapshot. The list behind it is fixed when the method returns; changing the VFS later
does not change a view a script already holds.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local before = vfs:keys()
vfs:removeResolvedFile("textures/tx_wood_02.dds")
assert(#before == 5 and #vfs:keys() == 4)
```

### What they cost

- `vfs:keys()` sorts the keys once and packs them into one block of memory. Later calls hand out
  the same block until the VFS changes, so asking again costs nothing.
- `entries`, `pathsMatching` and `pathsWith` collect and sort their entries when called. Each
  access to an item then builds a small `{ key, file }` table and a new `VfsFile` handle, so
  `view[1]` twice gives two different tables: keep the item in a local rather than indexing again.
- `toTable()` pushes every item at once. It is what a view saves you from; use it when a table is
  what you need.

`cargo bench -p vfstool_lib --all-features --bench luau_boundary` measures each of these against
the tables of the 0.10 binding; [Platforms and performance](@/docs/compatibility.md) has the
numbers.

### Types

Each view is declared with its item type, so a strict script measures, indexes and iterates one
without `toTable()`: `#keys` is a `number`, `keys[i]` a `string?`, and `for _, entry in entries`
types `entry` as `{ key: string, file: dream_vfs_VfsFile }`. The declarations are on [Type
definitions](@/docs/luau/types.md).

## Tree

`vfs:tree()`, `vfs:findByRegex()` and `vfs:remaining()` return a `Tree`: a directory tree, sorted,
walked depth first, a directory's files before its subdirectories. It is a stream rather than a
sequence: it can be iterated and counted, but not indexed.

| Operation | Result |
|---|---|
| `for index, row in tree do` | One `{ dir: string, file: VfsFile }` row per file, `dir` being the directory it sits in |
| `tree:count()` | The number of files |
| `tree:toTable()` | The nested tree as plain tables |
| `#tree` | An error: `attempt to get length of a dream.vfs.Tree value` |
| `tree[i]` | `nil` |

Every loop gets its own position, so one tree can be walked any number of times, and in nested
loops.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture", "mods/Door Fix" })
local tree = vfs:tree()
assert(tree:count() == 5)

local rows = {}
for _, row in tree do
    table.insert(rows, row.dir .. " | " .. row.file:path())
end
assert(rows[1] == "Data Files | Data Files/Extras.zip")
assert(rows[2] == "Data Files/meshes/x | mods/Door Fix/meshes/x/ex_door.nif")
assert(rows[5] == "Data Files/textures | mods/Wood Retexture/textures/tx_wood_02.dds")
```

`dir` comes from the tree the method built, so it depends on `relative`: under `Data Files`, with
the keys' own directories, by default; under `/`, with the files' host paths, when `relative` is
`false`. Files from an archive sit under a directory named after the archive.

### toTable

`tree:toTable()` is `{ [root] = node }`, where the root is `Data Files` or `/` and each node is:

| Field | Type |
|---|---|
| `files` | An array of file rows, sorted |
| `subdirs` | `{ [name] = node }`, one per subdirectory |

A file row is:

| Field | Type | Meaning |
|---|---|---|
| `path` | `string` | `file:path()` |
| `isLoose` | `boolean` | `file:isLoose()` |
| `isArchive` | `boolean` | `file:isArchive()` |
| `parentArchivePath` | `string?` | `file:parentArchivePath()` |
| `parentArchiveName` | `string?` | `file:parentArchiveName()` |
| `file` | `VfsFile` | The file itself |

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local root = vfs:tree():toTable()["Data Files"]
assert(root.files[1].path == "Data Files/Extras.zip")
local textures = root.subdirs.textures.files
assert(#textures == 3 and textures[1].isLoose and textures[3].file:readAll() == "new wood 2")
```
