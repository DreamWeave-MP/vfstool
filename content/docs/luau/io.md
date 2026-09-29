+++
title = "Writing files and host I/O"
description = "The lua-write feature: a write root, writeFile, openWrite and the Writer, mkdir, remove and rename that stay under it and register what they wrote. The lua-host feature: the host table, files and directories on any host path. The safety rules, and what each costs."
weight = 35

[extra]
kind = "api"
+++

Luau has no `io` library. With the `lua` feature alone, scripts read files through
[`VfsFile`](@/docs/luau/files.md) and write nothing. Two more Cargo features add writing, each a
deliberate grant by the host that builds the crate:

| Feature | Adds | For |
|---|---|---|
| `lua-write` | A write root on the VFS, `vfs:writeFile`, `vfs:openWrite`, `vfs:mkdir`, `vfs:remove`, `vfs:rename`, `vfs:writeRoot`, `vfs:setWriteRoot`, and the `writeRoot` constructor option | Scripts that produce files that belong in the VFS: a generated texture, a patched mesh, a merged plugin |
| `lua-host` | `vfstool.host`, files and directories on any host path | Tools with the run of the machine, like the command line itself |

Both add the `Writer` type. Neither is on by default, and a runtime built without one has none of
its members: `vfs.writeFile` is `nil`, `vfstool.host` is `nil`, and `writeRoot` is an unknown
option. The examples run against the [example install](@/docs/luau/module.md#the-example-install).

## Writes into the VFS

### The write root

{{ api_signature(value="VFS.fromDirectories(dirs, { archives?, writeRoot = dir })") }}

{{ api_signature(value="vfs:writeRoot() -> string?") }}

{{ api_signature(value="vfs:setWriteRoot(dir: string)") }}

Every write goes to `<writeRoot>/<key>`. A VFS without a write root refuses to write:
`dream.vfs: writeFile: this VFS has no write root (build it with writeRoot, or call
vfs:setWriteRoot)`. The three `fromDirectories` constructors take it as an option; a VFS built any
other way gets one from `setWriteRoot`. The directory is created when the first write needs it,
and need not be one of the VFS's data directories; when it is the highest one, what scripts write
is exactly what OpenMW would load.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" }, { writeRoot = "out" })
assert(vfs:writeRoot() == "out")
local plain = vfstool.VFS.fromDirectories({ "Data Files" })
assert(plain:writeRoot() == nil)
local ok, err = pcall(plain.writeFile, plain, "a.txt", "x")
assert(not ok and err:find("has no write root", 1, true))
```

### Keys stay under the root

A key that could leave the write root is refused before anything touches the disk:
`dream.vfs: writeFile: key '../x' escapes the write root (a key is relative, with no '..', root,
drive letter or NUL)`. Refused: an empty key, `..` or `.` components, a leading `/` or `\`, a
drive letter (`C:\x`), a NUL byte. `\` is a separator, as in every key. The file is written under
the key's own spelling (`Textures/Tx_New.dds` makes `out/Textures/Tx_New.dds`), and registered
under the normalized key.

`remove` and `rename` only ever touch `<writeRoot>/<key>`: a key whose file is somewhere else in
the VFS is not theirs, and they say so: `dream.vfs: remove: 'textures/tx_stone.dds' is not under
the write root`.

### writeFile

{{ api_signature(value="vfs:writeFile(key: string, data: buffer | string, options: { offset: number?, append: boolean?, create: boolean? }?) -> VfsFile") }}

Writes `data`, a buffer or a string, to `<writeRoot>/<key>`, creating the directories on the
way, then makes that file the winner of `key` (`setWinnerLooseFile`), so every later read sees
it, and returns its handle.

| Option | Default | Effect |
|---|---|---|
| none | | The file is replaced by `data` |
| `offset` | | `data` is written at `offset`; the rest of the file stays. Past the end, the gap is zeros |
| `append` | `false` | `data` goes after the current end |
| `create` | `true` | With `false`, a file that does not exist is an error instead of being made |

`offset` and `append` together are an error. As with every winner-only mutation, the key's
previous provider stack is discarded: `vfs:providersFor(key)` afterwards lists the written file
alone, with the directory it sits in as its source.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" }, { writeRoot = "out" })
local written = vfs:writeFile("textures/tx_wood_01.dds", "painted wood")
assert(written:path() == "out/textures/tx_wood_01.dds")
assert(vfs:getFile("textures/tx_wood_01.dds"):readAll() == "painted wood")
assert(#vfs:providersFor("textures/tx_wood_01.dds") == 1)

vfs:writeFile("textures/tx_wood_01.dds", "PAINTED", { offset = 0 })
vfs:writeFile("textures/tx_wood_01.dds", "!", { append = true })
assert(vfs:getFile("textures/tx_wood_01.dds"):readAll() == "PAINTED wood!")

local buf = buffer.create(4)
buffer.writeu32(buf, 0, 0xDDS0)
vfs:writeFile("textures/generated/tx_new.dds", buf)
assert(vfs:getFile("textures/generated/tx_new.dds"):size() == 4)

local ok, err = pcall(vfs.writeFile, vfs, "../outside.txt", "x")
assert(not ok and err:find("escapes the write root", 1, true))
ok, err = pcall(vfs.writeFile, vfs, "textures", "x")
assert(not ok and err:find("the VFS refused key 'textures'", 1, true))
```

The last error: `textures` is a directory of the VFS, and a file and a directory cannot share a
name. The file was written, but the VFS did not take it as a key.

### openWrite and the Writer

{{ api_signature(value="vfs:openWrite(key: string, options: { append: boolean?, truncate: boolean? }?) -> Writer") }}

A buffered writer over `<writeRoot>/<key>`, for a file produced in pieces. The file is created,
and emptied unless `append` or `truncate = false`; an `append` writer starts at the end. The key
is registered when the writer is closed, not before, so a script that fails halfway leaves the
VFS as it was.

| Method | Does |
|---|---|
| `write(data: buffer \| string, offset?, length?) -> number` | Writes `data`, or `length` bytes of it from `offset`, at the position and moves past them; returns the count |
| `writeAt(position, data, offset?, length?) -> number` | Writes at `position` without moving; an error on an `append` writer, whose writes always go to the end |
| `seek(position)` | Moves; past the end, the next write extends the file |
| `tell() -> number` | The position |
| `flush()` | Writes the buffer out |
| `truncate(length)` | Cuts or extends the file to `length`; a position past that moves to it |
| `close() -> VfsFile?` | Flushes, closes, makes the file the key's winner and returns it; `nil` from a second `close` and from a `host.openWrite` writer |

A closed writer refuses every method but `tell`: `dream.vfs: Writer.write: the writer over
out/log.txt is closed`. A writer that is collected without `close` still flushes, and registers
nothing. `tostring(writer)` is `dream.vfs.Writer(<path>)`.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files" }, { writeRoot = "out" })
local log = vfs:openWrite("logs/run.txt")
assert(log:write("hello ") == 6 and log:tell() == 6)
log:write(buffer.fromstring("--world--"), 2, 5)
log:writeAt(0, "J")
assert(not vfs:contains("logs/run.txt"))
local file = log:close()
assert(file:readAll() == "Jello world" and vfs:getFile("logs/run.txt") ~= nil)

local more = vfs:openWrite("logs/run.txt", { append = true })
assert(more:tell() == 11)
more:write("!")
more:close()
assert(vfs:getFile("logs/run.txt"):readAll() == "Jello world!")
```

### mkdir, remove and rename

{{ api_signature(value="vfs:mkdir(key: string)") }}

{{ api_signature(value="vfs:remove(key: string)") }}

{{ api_signature(value="vfs:rename(from: string, to: string)") }}

`mkdir` creates `<writeRoot>/<key>` and its parents. `remove` deletes `<writeRoot>/<key>`, a
file or a whole directory, and drops the winners that file or directory held from the VFS; a key
with nothing under the write root is an error. `rename` moves `<writeRoot>/<from>` to
`<writeRoot>/<to>`, creating `to`'s parents, and moves the winners with it, a directory's
included.

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files" }, { writeRoot = "out" })
vfs:writeFile("staging/a.txt", "a")
vfs:writeFile("staging/deep/b.txt", "b")
vfs:rename("staging", "final")
assert(not vfs:contains("staging/a.txt") and vfs:getFile("final/deep/b.txt"):readAll() == "b")
vfs:remove("final")
assert(not vfs:contains("final/a.txt") and not vfs:contains("final/deep/b.txt"))
local ok, err = pcall(vfs.remove, vfs, "textures/tx_stone.dds")
assert(not ok and err:find("is not under the write root", 1, true))
```

## Host paths

{{ api_signature(value="vfstool.host") }}

With `lua-host`, a frozen table of functions over host paths: byte strings, exact on Unix,
converted where the OS insists on Unicode, as everywhere in the module. Nothing here consults or
changes a VFS.

| Function | Does |
|---|---|
| `readFile(path) -> buffer` | The whole file, one copy from the file into a new buffer |
| `readFileString(path) -> string` | The whole file as a string |
| `readAt(path, offset, length) -> buffer` | `length` bytes from `offset`, fewer at the end; `offset` past the end is an error |
| `writeFile(path, data: buffer \| string, { append? }?) -> number` | Replaces the file with `data`, or appends; returns the count. Parents are not created |
| `open(path) -> Reader` | A [reader](@/docs/luau/files.md#open-and-the-reader) over the file, mapped |
| `openWrite(path, { append?, truncate? }?) -> Writer` | A [writer](#openwrite-and-the-writer) whose `close` returns `nil` |
| `stat(path) -> { size, isFile, isDir, modified, readonly }?` | Metadata, through symbolic links; `nil` when nothing is there. `modified` is seconds since the Unix epoch, `nil` where the OS has none |
| `exists(path) -> boolean` | Whether something is there |
| `list(path, { recursive? }?) -> HostEntries` | The entries of a directory, or of its whole tree, as a [sequence view](@/docs/luau/views.md) of `{ path, isDir, size }` rows sorted by path; `path` is the entry's full host path |
| `mkdir(path, { recursive? }?)` | Creates a directory; with `recursive`, its parents too |
| `remove(path, { recursive? }?)` | Deletes a file or an empty directory; with `recursive`, a directory and everything in it |
| `rename(from, to)` | Moves or renames |
| `copy(from, to) -> number` | Copies a file; returns the bytes copied |
| `canonicalize(path) -> string` | The absolute path with links resolved |

```lua
local host = require("@dream/vfs").host

host.mkdir("scratch/notes", { recursive = true })
assert(host.writeFile("scratch/notes/a.txt", "abc") == 3)
host.writeFile("scratch/notes/a.txt", buffer.fromstring("def"), { append = true })
assert(host.readFileString("scratch/notes/a.txt") == "abcdef")
assert(buffer.len(host.readAt("scratch/notes/a.txt", 3, 100)) == 3)

local st = host.stat("scratch/notes/a.txt")
assert(st.size == 6 and st.isFile and not st.isDir)
assert(host.stat("scratch/nothing") == nil)

local rows = host.list("scratch", { recursive = true })
assert(#rows == 2 and rows[1].path == "scratch/notes" and rows[1].isDir)
assert(rows[2].path == "scratch/notes/a.txt" and rows[2].size == 6)

host.copy("scratch/notes/a.txt", "scratch/b.txt")
host.rename("scratch/b.txt", "scratch/c.txt")
host.remove("scratch", { recursive = true })
assert(not host.exists("scratch"))
```

Errors carry the system's words after the module's prefix: `dream.vfs: No such file or directory
(os error 2)`. A `readAt` offset past the end is `dream.vfs: host.readAt: offset 1001 past the end
(size 1000)`.

## Cost

`cargo bench -p vfstool_lib --all-features --bench luau_boundary` has a `luau_io` group that
measures each read and write next to its plain Rust equivalent. One `--quick` run on the
development machine (Linux, an NVMe disk, the files in the page cache):

| Case | Luau, through the module | Plain Rust | Rust equivalent |
|---|---|---|---|
| `readAt` 4 KiB from an 8 MiB loose file, per call | 170 ns | 356 ns | `File::read_at` (a system call per read; the module reads from its map) |
| `readAt` 1 MiB | 33.9 µs | 53.2 µs | `File::read_at` |
| `readRange` 64 KiB (a new buffer) | 3.91 µs | 2.36 µs | A `Vec` and a memcpy |
| Reader, 16 KiB chunks over 8 MiB (512 calls) | 244 µs | 536 µs | A `File::read` loop |
| `vfs:writeFile` 1 MiB | 214 µs | 209 µs | `fs::write` |
| `host.readFile` 1 MiB (a new buffer) | 83.2 µs | 58.9 µs | `fs::read` |
| `require` of a 200-line module, per instance, template cached | 14.4 µs | | One `lua_clonefunction` and a run |
| The same with the template cache cleared before each | 642 µs | | A compile and a run |

The positional reads and the reader come out ahead of the system-call loops because the module
reads from its memory map: after the first call the copy is a memcpy and no call enters the
kernel. A new buffer (`readRange`, `host.readFile`) costs Luau's allocation of it on top of the
copy, and the 1 MiB write is the write itself; the boundary adds a call and a bounds check.
