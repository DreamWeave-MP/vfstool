# Embedded Luau API

`vfstool_lib` exposes its promoted stable API to Luau through the `lua` feature as an
[l3i](https://github.com/DreamWeave-MP/l3i) extension: `vfstool_lib::lua::VfsExtension`, id
`dream.vfs`, module `@dream/vfs`. It is not a `cdylib` plugin ABI, it never creates a VM, and it
never installs a global. The host composes the extension into its `RuntimePlan` (together with
dream-path's `PathExtension`, which it requires) and every runtime made from that plan can
`require("@dream/vfs")`.

```rust
use l3i::Runtime;
use l3i::extension::{RuntimePlan, RuntimePolicy};
use vfstool_lib::lua::{MODULE, MODULE_NAME, VfsExtension};

// Optional: expose the module as the historical `vfstool` global as well.
let policy = RuntimePolicy::new().compat_global(MODULE, MODULE_NAME);
let plan = RuntimePlan::builder()
    .policy(policy)
    .extension(dream_path::lua::PathExtension)
    .extension(VfsExtension)
    .finalize()?;
let runtime = Runtime::from_plan(&plan)?;
runtime.exec(r#"local vfstool = require("@dream/vfs") print(vfstool.VFS.new():len())"#)?;
```

Enable the binding layer with:

```toml
vfstool_lib = { version = "0.11", features = ["lua"] }
```

A host that built a `VFS` in Rust hands it to scripts with `vfstool_lib::lua::Vfs::push`. The
plan's `type_definitions()` is the `.d.luau` for the whole composition, and the crate's tests
type check a strict script against it (behind the `luau-analysis` feature, which builds Luau's
analysis frontend), so the declared API and the runtime cannot drift apart.

## Scope

The Luau surface binds the promoted stable API and methods on its stable associated types:

- `VFS`
- `VfsFile`
- `VfsProvider`
- `LayerIndex`
- `ConflictIndex`
- lock/provenance/drift/semantic-conflict reports reachable from `LayerIndex`
- run-workflow helpers
- path/glob helpers
- semantic `analyzePair`

It deliberately does **not** bind:

- `experimental::*`
- solver, policy, or knowledge-base helpers
- low-level archive internals
- Rust iterators or parallel iterators

Reports are plain Lua tables. Long-lived mutable structures are userdata. Lists a script walks
are sequence views (below).

## Conventions

- Paths are strings read as bytes. No UTF-8 is required anywhere: a key spelled with bytes that
  are not UTF-8 is a key, and `file:path()` returns the exact bytes of the host path or entry name.
- A VFS key argument may be spelled any way (`Textures\Foo.DDS`); it is normalized once, into a
  reusable scratch buffer, and looked up without allocating.
- Functions, methods, and table fields are camelCase, and so are enum-like string values, for
  example `"looseDir"`, `"archive"`, `"ini"`, `"cosmeticOnly"`, `"winnerHashChanged"`.
- Indices are 1-based: `sourceIndex`, `providerIndex`, `winnerIndex`, the results of
  `sourcesContaining` and `sourceIdForPath`, and the arguments of `sourceById` and
  `providerOriginalPath`.
- Byte sizes (`size`, `winnerSize`) are Luau integers (`4i`); counts and indices are numbers.
- Option tables are strict: an unknown key is an error, so a misspelt option never becomes a
  silently ignored default.
- Luau has no `io` library; read and write files through the bindings or the host.
- Constructors that take directories expect arrays: `{ "/data/base", "/data/mod" }`.
- Archive loading still depends on `beth-archives` / `zip` Cargo features.
- `VFS` stores provider stacks low priority to high priority and caches the resolved winner map.
- Winner-only and stack-preserving mutation use different names. If a method says it removes only the
  winner, it reveals the next provider; if it says it removes the resolved file, it discards the stack.

### Sequence views

`keys`, `entries`, `pathsMatching`, `pathsWith`, `providersFor`, `providerRecordsFor`,
`layer:keys`, `layer:duplicateKeys`, and `filesFromArchive` return sequence views over the native
collection instead of copying it into a table: `#items`, `items[i]` (1-based, `nil` past the end),
`for index, item in items do`, and `items:toTable()` for the plain table the previous binding
returned. Only the item a script touches is pushed. They are userdata, so `ipairs`, `table.sort`,
and friends want `items:toTable()`; `vfstool.serialize` accepts a view directly. Each view is
declared with its element type (`Keys` holds `string`, `Entries` holds
`{ key: string, file: VfsFile }`, `Providers` holds `{ source, file }`, `ProviderRecords` holds
provider record rows), so a strict script measures, indexes, and iterates a view without
`toTable()`: `#keys`, `keys[i]` is `string?`, and `for _, entry in entries` types `entry`.

`tree`, `findByRegex`, and `remaining` return a `Tree` stream: `for _, row in tree do` yields
`{ dir = string, file = VfsFile }` rows (typed as such) in sorted, depth-first order,
`tree:count()` is the number of rows, and `tree:toTable()` is the nested
`{ [root] = { files = {...}, subdirs = {...} } }` shape.

## Top-level functions

```lua
vfstool.normalizeHostPath(path) -> string
vfstool.normalizeHostPathInPlace(path) -> string   -- same result; kept for compatibility
vfstool.pathGlobMatches(glob, path) -> boolean
vfstool.sourceGlobMatches(glob, sourcePath) -> boolean

vfstool.analyzePair(path, leftBytes, rightBytes) -> {
  assetClass = string,
  delta = { kind = string, changeSummary = { string }? },
}
```

Run workflow:

```lua
count, snapshot = vfstool.runSetup(vfs, mergedDir, useHardlinks)
count, metadataSnapshot = vfstool.runSetupTracked(vfs, mergedDir, useHardlinks)
snapshot = vfstool.snapshotDirectory(dir)             -- snapshot:len()
metadataSnapshot = vfstool.snapshotDirectoryMetadata(dir)
changed = vfstool.changedFiles(dir, snapshot)         -- { string }
changed = vfstool.changedFilesMetadata(dir, metadataSnapshot)
copied = vfstool.runFinalize(mergedDir, outputDir, snapshot)   -- { { relativePath, destinationPath } }
copied = vfstool.runFinalizeTracked(mergedDir, outputDir, metadataSnapshot)
```

`runSetup` may hardlink loose files into the merged directory. Child tools that edit files in place
can mutate the original source files through those hardlinks. Use `false` for `useHardlinks` if the
child tool is not hardlink-safe. This warning is part of the API, not decorative prose.

With `serialize` enabled:

```lua
vfstool.serialize(value, "json" | "yaml" | "toml") -> string
```

Tables whose keys are exactly `1..n` (and empty tables) serialize as arrays, other tables as objects
with string keys, sequence views as arrays of their `toTable()` rows, a tree stream as its
`toTable()` shape, and a `VfsFile` as its file row's fields (`path`, `isLoose`, `isArchive`,
`parentArchivePath`, `parentArchiveName`); strings must be UTF-8.

## `VFS`

```lua
vfs = vfstool.VFS.new()
vfs = vfstool.VFS.fromDirectories({ dir1, dir2 }, { archives = { "base.bsa" } })
vfs, conflicts = vfstool.VFS.fromDirectoriesWithConflictIndex({ dir1, dir2 })
vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex({ dir1, dir2 })

vfs:len() -> number                       -- O(1)
vfs:isEmpty() -> boolean
vfs:keys() -> Keys                        -- sorted sequence view of strings
vfs:entries() -> Entries                  -- sorted sequence view of { key = string, file = VfsFile }
vfs:getFile(path) -> VfsFile | nil
vfs:contains(path) -> boolean
vfs:findByRegex(pattern, relative?) -> Tree
vfs:remaining(filterPath, replacementsOnly, allDirs, relative?) -> Tree
vfs:pathsMatching(substring) -> Entries
vfs:pathsWith(prefix) -> Entries

vfs:setWinnerFile(key, file) -> VfsFile | nil
vfs:setWinnerLooseFile(key, physicalPath) -> VfsFile | nil
vfs:pushDirectory(path) -> nil
vfs:pushArchive(path) -> boolean          -- with an archive feature
vfs:pushProvider(key, provider) -> boolean
vfs:removeWinner(key) -> VfsProvider | nil
vfs:removeResolvedFile(key) -> VfsFile | nil
vfs:removeProvider(key, sourcePath) -> { { source = { path, kind }, file = VfsFile } }
vfs:removeSource(sourcePath) -> { { key = string, provider = { source, file } } }
vfs:removeProviderPrefix(prefix) -> { { key = string, provider = { source, file } } }
vfs:removeResolvedPrefix(prefix) -> { { key = string, file = VfsFile } }
vfs:removeResolvedMatchingGlob(glob) -> { { key = string, file = VfsFile } }

vfs:tree(relative?) -> Tree
vfs:display(relative?) -> string
vfs:dumpToDirectory(dir, useHardlinks) -> number
vfs:collapseInto(dest, opts) -> nil
vfs:extractFile(vfsPath, destDir) -> string | nil
vfs:diffDirectory(dir) -> { conflicts = { { key, incoming, current } }, additions = { { key, file } } }

vfs:providerRecordsFor(path) -> ProviderRecords   -- sequence view of provider record rows
vfs:providersFor(path) -> Providers | nil         -- sequence view of { source, file } rows
vfs:explain(path) -> { key, winner, overridden } | nil
vfs:duplicates(pattern?) -> { entries = { { key, providers, winnerIndex } } }
vfs:archives() -> { ArchiveInfo }
vfs:archiveEntries(archive) -> { ArchiveEntry }
vfs:filesFromArchive(archive) -> Keys
vfs:sourceContributions() -> { sources = { ... } }
vfs:materializationPlan(dest, opts) -> { actions = {...}, issues = {...} }
vfs:layerIndex() -> LayerIndex
vfs:serializeTree(relative?, format) -> string      -- with `serialize`
```

`collapseInto` and `materializationPlan` options (all optional, no other keys):

```lua
{
  allowCopying = false,
  extractArchives = false,
  useSymlinks = false,
}
```

## `VfsFile`

```lua
file = vfstool.VfsFile.from(path)
file:isLoose() -> boolean
file:isArchive() -> boolean
file:path() -> string                 -- exact bytes
file:fileName() -> string | nil
file:fileStem() -> string | nil
file:parentArchivePath() -> string | nil
file:parentArchiveName() -> string | nil
file:readAll() -> string
file:readInto(buffer, offset?) -> number   -- bytes written, at most the space after offset
```

`readInto` streams the file straight into a Luau buffer and returns how many bytes it wrote; a
file larger than the space left is truncated to it, so compare the result with the space to detect
that. `readAll` is the convenient form and copies once more.

## `VfsProvider`

```lua
provider = vfstool.VfsProvider.new({ path = dir, kind = "looseDir" }, file)
provider:source() -> { path = string, kind = string }
provider:file() -> VfsFile
```

## `LayerIndex`

```lua
layer = vfstool.LayerIndex.fromFileLists({
  { source = { path = "base", kind = "looseDir" }, files = { "a.txt" } },
})

layer:keys() -> Keys                     -- sorted sequence view
layer:sources() -> { { path = string, kind = string } }
layer:sourceIdForPath(path) -> number | nil       -- 1-based
layer:sourceById(id) -> table | nil               -- 1-based
layer:sourcesContaining(path) -> { number }       -- 1-based
layer:providerOriginalPath(sourceIndex, path) -> string | nil
layer:providerChain(path) -> { LayerProvider }
layer:duplicateKeys() -> Keys
layer:sourceContributions() -> table
layer:provenance(vfs, path, withHashes) -> table | nil
lock = layer:lockManifest(vfs)
layer:diffAgainstLock(vfs, lock) -> table
layer:semanticConflicts(vfs, opts?) -> table
```

Semantic conflict options:

```lua
{
  archiveHashMode = "disabled" | "winnerOnly" | "allProviders",
  includeSemanticDeltas = false,
}
```

## `VfsLock`

```lua
lock:schemaVersion() -> number
lock:entries() -> { { key, winnerSource, winnerKind, winnerHashBlake3, winnerSize, providerCount } }
lock:toTable() -> { schemaVersion, entries }
```

## `ConflictIndex`

```lua
conflicts = vfstool.ConflictIndex.fromDirectories({ dir1, dir2 })
conflicts = vfstool.ConflictIndex.fromFileLists({
  { source = "base", files = { "a.txt" } },
})
conflicts = vfstool.ConflictIndex.fromLayerIndex(layer)

conflicts:sources() -> { string }
conflicts:sourcesContaining(path) -> { number }   -- 1-based
conflicts:conflictsReport(relative?) -> table
conflicts:shadowedReport(relative?, listFiles?) -> table
conflicts:diffReport(sourceA, sourceB) -> table
```

## Report tables

Report table field names mirror the Rust report structs, in camelCase. Nested provider records
use this shape:

```lua
{
  sourceIndex = 1,
  source = { path = "/mods/foo", kind = "looseDir" },
  key = "textures/foo.dds",
  originalPath = "Textures/Foo.DDS",
  resolvedPath = "/mods/foo/Textures/Foo.DDS",
}
```

The Lua binding is intentionally boring: deterministic tables in, deterministic tables out. Clever
Lua magic belongs in the host application, not in the FFI seam.

## Measured

`cargo bench -p vfstool_lib --all-features --bench luau_boundary`: a 1000-file loose fixture
(`vfs`) and a 5000-file one (`vfsLarge`), the same frozen scripts against the previous mlua
binding and this one, both pinned to one core, minimum of five runs on a loaded machine. Per call,
or per materialised collection:

| script | mlua 0.12 | l3i |
|---|---:|---:|
| `getFile` hit (`textures/file_00006.dat`) | 400 ns | 250 ns |
| `getFile` hit, unnormalized spelling (`Textures\File_00006.DAT`) | 396 ns | 286 ns |
| `getFile` miss | 167 ns | 98 ns |
| `contains` hit / miss | 181 / 185 ns | 91 / 88 ns |
| `len` (5000 keys) | 82 ns | 53 ns |
| `keys` (5000 keys) | 1.33 ms (table) | 618 µs first call, 279 ns repeated (view); `:toTable()` 359 µs |
| `entries` (1000) | 823 µs (table) | 226 µs (view); `:toTable()` 620 µs |
| `pathsMatching` broad (1000 hits) / narrow (1 hit) | 1.03 ms / 79 µs | 262 µs / 65 µs |
| `pathsWith('textures')` (167 hits) | 141 µs | 41 µs |
| `providersFor` hit | 1.58 µs | 341 ns |
| `explain` hit | 2.84 µs | 1.99 µs |
| `tree` (1000 files) | 2.11 ms (nested tables) | 898 µs (stream); `:toTable()` 1.47 ms |
| `file:readAll()` 64 KiB | 19.6 µs | 19.0 µs |
| `file:readInto(buffer)` 64 KiB | n/a | 5.4 µs |
| `file:path()` | 164 ns | 105 ns |

## Migrating from the mlua binding (0.10)

- `lua::open(&Lua)` and `lua::register(&Lua)` are gone with `mlua`. Compose `lua::VfsExtension`
  (and dream-path's `PathExtension`) into the host's `RuntimePlan`; the `vfstool` global, if the
  host still wants one, is `RuntimePolicy::compat_global("@dream/vfs", "vfstool")`.
- `keys`, `entries`, `pathsMatching`, `pathsWith`, `providersFor`, `providerRecordsFor`,
  `layer:keys`, `layer:duplicateKeys`, and `filesFromArchive` are sequence views: `#`, `[i]`, and
  `for` work as before; `ipairs`, `table.*`, and other table-only code need `:toTable()`.
- `tree`, `findByRegex`, and `remaining` are a `Tree` stream; `tree:toTable()` is the old nested
  table.
- Indices are 1-based everywhere they used to be 0-based.
- `size` and `winnerSize` are Luau integers (`==` compares them with `4i`; `tonumber` converts).
- Unknown keys in option tables (`{ archives = ... }`, collapse options, semantic options, source
  metadata) are errors.
- Paths are bytes: a key that is not UTF-8 no longer raises a conversion error, and `file:path()`
  returns the exact bytes rather than a lossy string.
- Added: `file:readInto(buffer, offset?)`, `tree:count()`, `snapshot:len()`,
  `lock:toTable()` shape unchanged.
