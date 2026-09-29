+++
title = "Luau API"
description = "The @dream/vfs module scripts require: the VFS, its files, indexes, views and reports, the type definitions, and the extension a host composes to provide it."
template = "docs/section.html"
page_template = "docs/page.html"
sort_by = "weight"
weight = 100

[extra]
kind = "api"
hide_child_cards = true
+++

With the `lua` feature, vfstool_lib provides the Luau module `@dream/vfs`: the library's stable
API, the same code the command line runs, for scripts in any host built on
[l3i](https://github.com/DreamWeave-MP/l3i).

```lua
local vfstool = require("@dream/vfs")

local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture", "mods/Door Fix" })
local report = vfs:explain("meshes/x/ex_door.nif")
assert(report.winner.source.path == "mods/Door Fix")
assert(report.overridden[1].originalPath == "Meshes/x/Ex_Door.nif")

for _, entry in vfs:pathsWith("textures") do
    print(entry.key, entry.file:path())
end
```

| Page | Covers |
|---|---|
| [@dream/vfs](@/docs/luau/module.md) | Requiring it, the conventions, the path and glob helpers, `analyzePair`, the run workflow, `serialize`, and the errors |
| [VFS](@/docs/luau/vfs.md) | The constructors and every method: lookups, provider reports, changes, and writing it out |
| [VfsFile and VfsProvider](@/docs/luau/files.md) | Files, loose or archived: `size`, `readAll`, `readInto`, the positional `readAt`, `readRange` and `readAllBuffer`, the `Reader`, and providers |
| [Writing files and host I/O](@/docs/luau/io.md) | With `lua-write`, a write root and `writeFile`, `openWrite`, `mkdir`, `remove`, `rename`; with `lua-host`, the `host` table over any host path; the `Writer` |
| [LayerIndex, VfsLock and ConflictIndex](@/docs/luau/indexes.md) | Provenance, locks, drift, semantic conflicts, conflict reports and snapshots |
| [Sequence views and trees](@/docs/luau/views.md) | `Keys`, `Entries`, `Providers`, `ProviderRecords` and the `Tree` stream |
| [Report tables](@/docs/luau/reports.md) | Every table the module returns, field by field |
| [Type definitions](@/docs/luau/types.md) | The `.d.luau` declarations a plan generates |
| [require over the VFS](@/docs/luau/require.md) | `VfsRequireNavigator`: `require("./x")`, `.luaurc` and host aliases as VFS keys, and the template cache |
| [VfsExtension](@/docs/luau/extension.md) | The Rust side: the extension, its constants, the handles and views |

It binds the stable API only: `VFS`, `VfsFile`, `VfsProvider`, `LayerIndex`, `ConflictIndex`,
the reports reachable from them, the run workflow, the path and glob helpers, and `analyzePair`.
The `experimental` module, the archive internals, and Rust's iterators are not bound.

[Embedding Luau](@/docs/luau-hosts.md) walks through the host's side: the features, the
toolchain, composing the extension, and handing scripts a VFS.
