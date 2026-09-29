+++
title = "VfsFile and VfsProvider"
description = "The file handle behind every key, loose or in an archive, with readAll and readInto, and the provider that pairs a file with its source."
weight = 30

[extra]
kind = "api"
+++

The examples run against the [example install](@/docs/luau/module.md#the-example-install).

## VfsFile

A handle to one file: a loose file on disk, or an entry inside an archive the VFS opened. The
handle does not hold the file open; each read opens it again. `getFile`, `entries`, providers and
reports hand them out, and `VfsFile.from` makes one for a loose file.

### VfsFile.from

{{ api_signature(value="VfsFile.from(path: string) -> VfsFile") }}

A handle to the loose file at a host path. Nothing is checked until the file is read: a handle to
a file that does not exist reads as an error.

### isLoose and isArchive

{{ api_signature(value="file:isLoose() -> boolean") }}

{{ api_signature(value="file:isArchive() -> boolean") }}

Whether the file is on disk or inside an archive. Exactly one is `true`.

### path

{{ api_signature(value="file:path() -> string") }}

The host path of a loose file, or the entry's name inside its archive, byte for byte, spelled as
the directory or the archive spells it.

### fileName and fileStem

{{ api_signature(value="file:fileName() -> string?") }}

{{ api_signature(value="file:fileStem() -> string?") }}

The last component of `path()`, with and without its extension; `nil` when there is none, as for
`/`.

### parentArchivePath and parentArchiveName

{{ api_signature(value="file:parentArchivePath() -> string?") }}

{{ api_signature(value="file:parentArchiveName() -> string?") }}

For a file inside an archive, the archive's host path and its file name; `nil` for a loose file.
These two are converted to UTF-8, with any invalid bytes replaced.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files" }, { archives = { "Extras.zip" } })

local stone = vfs:getFile("textures/tx_stone.dds")
assert(stone:isLoose() and stone:path() == "Data Files/Textures/tx_stone.dds")
assert(stone:fileName() == "tx_stone.dds" and stone:fileStem() == "tx_stone")
assert(stone:parentArchivePath() == nil)

local extra = vfs:getFile("textures/tx_extra.dds")
assert(extra:isArchive() and extra:path() == "Textures/tx_extra.dds")
assert(extra:parentArchivePath() == "Data Files/Extras.zip")
assert(extra:parentArchiveName() == "Extras.zip")
```

### readAll

{{ api_signature(value="file:readAll() -> string") }}

The whole file, as a string of its bytes, read from disk or from the archive.

### readInto

{{ api_signature(value="file:readInto(buffer: buffer, offset: number?) -> number") }}

Reads the file straight into a Luau `buffer`, starting at `offset` (default 0), and returns how
many bytes it wrote: at most the space from `offset` to the end of the buffer. A file larger than
that space is cut off there, so compare the result with the file's size, or with the space, to tell.
Nothing is copied on the way, which makes it the cheaper of the two for large files.

`offset` must be a whole number. A negative one raises `dream.vfs: negative buffer offset`; one
past the end of the buffer raises `buffer access out of bounds`. An offset exactly at the end reads
nothing and returns 0.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local wood = vfs:getFile("textures/tx_wood_01.dds")
assert(wood:readAll() == "new wood")

local buf = buffer.create(16)
assert(wood:readInto(buf) == 8)
assert(buffer.readstring(buf, 0, 8) == "new wood")

assert(wood:readInto(buf, 12) == 4)
assert(buffer.readstring(buf, 12, 4) == "new ")

local missing = vfstool.VfsFile.from("Data Files/missing.dds")
assert(not pcall(missing.readAll, missing))
```

`tostring(file)` is `dream.vfs.VfsFile(<path>)`.

## VfsProvider

One provider of a key: a file and the source it comes from. `removeWinner` returns one, and
`pushProvider` takes one.

### VfsProvider.new

{{ api_signature(value="VfsProvider.new(source: { path: string, kind: string }, file: VfsFile) -> VfsProvider") }}

`source.path` is the data directory or archive the provider belongs to, and `source.kind` is
`"looseDir"` or `"archive"`. Both are required and nothing else is accepted.

### source and file

{{ api_signature(value="provider:source() -> Source") }}

{{ api_signature(value="provider:file() -> VfsFile") }}

The source as a `{ path, kind }` table, and the file.

```lua
local vfstool = require("@dream/vfs")

local file = vfstool.VfsFile.from("mods/Door Fix/meshes/x/ex_door.nif")
local provider = vfstool.VfsProvider.new({ path = "mods/Door Fix", kind = "looseDir" }, file)
assert(provider:source().kind == "looseDir")
assert(provider:file():readAll() == "fixed door")
assert(tostring(provider) == "dream.vfs.VfsProvider(mods/Door Fix/meshes/x/ex_door.nif from mods/Door Fix)")

local ok, err = pcall(vfstool.VfsProvider.new, { path = "mods/Door Fix", kind = "zip" }, file)
assert(not ok and err:find("unknown source kind 'zip'", 1, true))
```
