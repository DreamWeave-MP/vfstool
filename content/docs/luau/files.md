+++
title = "VfsFile and VfsProvider"
description = "The file handle behind every key, loose or in an archive: size, readAll and readInto, the positional readAt, readRange and readAllBuffer, the Reader for sequential parsing, and the provider that pairs a file with its source."
weight = 30

[extra]
kind = "api"
+++

The examples run against the [example install](@/docs/luau/module.md#the-example-install).

## VfsFile

A handle to one file: a loose file on disk, or an entry inside an archive the VFS opened.
`getFile`, `entries`, providers and reports hand them out, and `VfsFile.from` makes one for a
loose file. `readAll` and `readInto` open the file for each call; the positional reads and
`open` keep a backing on the handle instead, described under [readAt](#readat).

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
`/`. An archive entry's folders end at `\` as well as `/`, so a Morrowind BSA's
`meshes\x\ex_door.nif` is `ex_door.nif` on every platform.

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

### size

{{ api_signature(value="file:size() -> number") }}

The size in bytes. For a loose file, its metadata; for an entry in a TES3 BSA, a BA2 or a ZIP,
the uncompressed size the archive's index records, so nothing is read; for a TES4 BSA entry,
whose index holds the compressed size only, the entry is decompressed once and kept, as for
`readAt`. A file that cannot be reached is an error.

### readAll

{{ api_signature(value="file:readAll() -> string") }}

The whole file, as a string of its bytes, read from disk or from the archive.

### readAllBuffer

{{ api_signature(value="file:readAllBuffer() -> buffer") }}

The whole file as a new `buffer` of exactly its size: one copy, from the file's map or bytes
(see [readAt](#readat)) into the buffer.

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

### readAt

{{ api_signature(value="file:readAt(buffer: buffer, fileOffset: number, length: number?, bufferOffset: number?) -> number") }}

Copies `length` bytes of the file, from `fileOffset`, into the buffer at `bufferOffset`, and
returns how many it copied: `length`, or fewer only when the file ends first. `bufferOffset`
defaults to 0 and `length` to the space from there to the end of the buffer. No handle state is
involved: two `readAt`s in a row, or from two scripts, do not affect each other.

The first positional read gives the handle a *backing*, which every later `readAt`, `readRange`,
`readAllBuffer` and `open` on that handle uses:

| File | Backing | A read is |
|---|---|---|
| Loose | A private, read-only memory map of the file (a positional read on the handle when the OS refuses to map it, as for an empty file) | One memcpy |
| In an archive, compressed | The entry, decompressed once into memory | One memcpy |
| In an archive, stored uncompressed (a TES3 BSA entry, a ZIP entry with the `Stored` method) | None until `open` or `readAllBuffer`: `readAt` and `readRange` read the bytes straight from the archive | A read of the bytes asked for and nothing else |

A handle is what `getFile` returns; each call returns a new one, so a script that reads a file
many times keeps the handle in a local. The map or bytes are shared by every reader opened from
the handle and released with the last of them.

Every argument is checked before anything is copied, and the error names it:

| Mistake | Error |
|---|---|
| `fileOffset` past the end (equal to the size reads nothing and is fine) | `dream.vfs: readAt: fileOffset 1001 past the end (size 1000)` |
| A negative offset or length | `dream.vfs: readAt: fileOffset -1 is negative`, `length -3 is negative` |
| `bufferOffset` past the end of the buffer | `dream.vfs: readAt: bufferOffset 65 past the end of the buffer (size 64)` |
| `length` beyond the space | `dream.vfs: readAt: length 65 does not fit the buffer (space 64 after bufferOffset 0)` |
| A fraction | `dream.vfs.VfsFile.readAt: bad argument #2 (expected integer): Lua stack index 3: number 1.5 is not an exact i64` |

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local wood = vfs:getFile("textures/tx_wood_01.dds")
assert(wood:size() == 8)

local buf = buffer.create(16)
assert(wood:readAt(buf, 4) == 4)
assert(buffer.readstring(buf, 0, 4) == "wood")
assert(wood:readAt(buf, 0, 3, 8) == 3)
assert(buffer.readstring(buf, 8, 3) == "new")
assert(wood:readAt(buf, 8) == 0)

local ok, err = pcall(wood.readAt, wood, buf, 9)
assert(not ok and err:find("fileOffset 9 past the end (size 8)", 1, true))
```

### readRange

{{ api_signature(value="file:readRange(fileOffset: number, length: number) -> buffer") }}

A new buffer of `length` bytes from `fileOffset`, shorter only when the file ends first. The
same checks as `readAt`, named `readRange`.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local wood = vfs:getFile("textures/tx_wood_01.dds")
assert(buffer.tostring(wood:readRange(4, 4)) == "wood")
assert(buffer.len(wood:readRange(6, 100)) == 2)
assert(buffer.tostring(wood:readAllBuffer()) == "new wood")
```

### open and the Reader

{{ api_signature(value="file:open() -> Reader") }}

A sequential reader over the file's backing: a position, and the map or bytes the handle keeps.
A parse that walks a 200 MB file in 16 KiB chunks allocates nothing per call. Several readers
over one handle share the backing and have their own positions.

| Method | Does |
|---|---|
| `read(length: number) -> buffer` | The next `length` bytes as a new buffer, fewer at the end; moves past them |
| `readInto(buffer: buffer, bufferOffset: number?, length: number?) -> number` | Copies the next bytes into the buffer, at most `length` (default the space after `bufferOffset`, default 0); returns the count, 0 at the end; moves past them |
| `seek(position: number)` | Moves to `position`, from 0 to the size inclusive |
| `skip(count: number)` | Moves `count` bytes forward, at most to the end |
| `tell() -> number` | The position |
| `size() -> number` | The file's size |
| `close()` | Releases the backing; every later read or move is an error |

A closed reader answers `tell` and `size` and refuses the rest: `dream.vfs: Reader.read: the
reader over Data Files/Textures/tx_stone.dds is closed`. A reader that is collected without
`close` releases the backing then. Moving past the end is an error that names the argument:
`dream.vfs: Reader.seek: position 301 past the end (size 300)`, `dream.vfs: Reader.skip: count 301
past the end (position 0, size 300)`; `readInto`'s window is checked like `readAt`'s.
`tostring(reader)` is `dream.vfs.Reader(<path>)`.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
local reader = vfs:getFile("textures/tx_wood_01.dds"):open()
assert(reader:size() == 8 and reader:tell() == 0)
assert(buffer.tostring(reader:read(3)) == "new")
reader:skip(1)
local chunk = buffer.create(16)
assert(reader:readInto(chunk) == 4 and buffer.readstring(chunk, 0, 4) == "wood")
assert(reader:readInto(chunk) == 0)
reader:seek(4)
assert(buffer.tostring(reader:read(100)) == "wood")
reader:close()
assert(not pcall(reader.read, reader, 1))
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
