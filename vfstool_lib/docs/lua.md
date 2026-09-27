# Embedded Lua API

`vfstool_lib` exposes optional embedded Lua bindings through the `lua` feature. This is not a
`cdylib` plugin ABI and it does not install a global module by itself. Host applications create a
Lua state and register the table they want.

```rust
let lua = mlua::Lua::new();
vfstool_lib::lua::register(&lua)?; // installs global `vfstool`
```

Or avoid globals:

```rust
let lua = mlua::Lua::new();
let module = vfstool_lib::lua::open(&lua)?;
lua.globals().set("vfstool", module)?;
```

Enable the binding layer with:

```toml
vfstool_lib = { version = "1", features = ["lua"] }
```

The runtime is [Luau](https://luau.org): the `lua` feature enables `mlua`'s `luau` backend, which
is built from source, so there is nothing to install and every host gets the same runtime. Hosts
that register several libraries into one state (`vfstool` and `jess`, say) must use the same `mlua`
version.

The binding is embedded either way. There is no dynamic Lua module pretending to be a stable C ABI.
Good.

## Scope

The Lua surface binds the promoted stable API and methods on its stable associated types:

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

Reports are plain Lua tables. Long-lived mutable structures are userdata.

## Conventions

- Paths are strings.
- Functions, methods, and table fields are camelCase, and so are enum-like string values, for
  example `"looseDir"`, `"archive"`, `"ini"`, `"cosmeticOnly"`, `"winnerHashChanged"`.
- Luau has no `io` library; read and write files through the bindings or the host.
- Constructors that take directories expect arrays: `{ "/data/base", "/data/mod" }`.
- Archive loading still depends on `beth-archives` / `zip` Cargo features.
- `VFS` stores provider stacks low priority to high priority and caches the resolved winner map.
- Winner-only and stack-preserving mutation use different names. If a method says it removes only the
  winner, it reveals the next provider; if it says it removes the resolved file, it discards the stack.

## Top-level functions

```lua
vfstool.normalizeHostPath(path) -> string
vfstool.normalizeHostPathInPlace(path) -> string
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
snapshot = vfstool.snapshotDirectory(dir)
metadataSnapshot = vfstool.snapshotDirectoryMetadata(dir)
changed = vfstool.changedFiles(dir, snapshot)
changed = vfstool.changedFilesMetadata(dir, metadataSnapshot)
copied = vfstool.runFinalize(mergedDir, outputDir, snapshot)
copied = vfstool.runFinalizeTracked(mergedDir, outputDir, metadataSnapshot)
```

`runSetup` may hardlink loose files into the merged directory. Child tools that edit files in place
can mutate the original source files through those hardlinks. Use `false` for `useHardlinks` if the
child tool is not hardlink-safe. This warning is part of the API, not decorative prose.

With `serialize` enabled:

```lua
vfstool.serialize(value, "json" | "yaml" | "toml") -> string
```

## `VFS`

```lua
vfs = vfstool.VFS.new()
vfs = vfstool.VFS.fromDirectories({ dir1, dir2 }, { archives = { "base.bsa" } })
vfs, conflicts = vfstool.VFS.fromDirectoriesWithConflictIndex({ dir1, dir2 })
vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex({ dir1, dir2 })

vfs:len() -> integer
vfs:isEmpty() -> boolean
vfs:keys() -> { string }                 -- sorted
vfs:entries() -> { { key = string, file = VfsFile } }
vfs:getFile(path) -> VfsFile | nil
vfs:contains(path) -> boolean
vfs:findByRegex(pattern, relative?) -> table
vfs:remaining(filterPath, replacementsOnly, allDirs, relative?) -> table
vfs:pathsMatching(substring) -> { { key = string, file = VfsFile } }
vfs:pathsWith(prefix) -> { { key = string, file = VfsFile } }

vfs:setWinnerFile(key, file) -> VfsFile | nil
vfs:setWinnerLooseFile(key, physicalPath) -> VfsFile | nil
vfs:pushDirectory(path) -> nil
vfs:pushArchive(path) -> boolean
vfs:pushProvider(key, provider) -> boolean
vfs:removeWinner(key) -> VfsProvider | nil
vfs:removeResolvedFile(key) -> VfsFile | nil
vfs:removeProvider(key, sourcePath) -> { VfsProvider }
vfs:removeSource(sourcePath) -> { { key = string, provider = table } }
vfs:removeProviderPrefix(prefix) -> { { key = string, provider = table } }
vfs:removeResolvedPrefix(prefix) -> { { key = string, file = VfsFile } }
vfs:removeResolvedMatchingGlob(glob) -> { { key = string, file = VfsFile } }

vfs:tree(relative?) -> table
vfs:display(relative?) -> string
vfs:dumpToDirectory(dir, useHardlinks) -> integer
vfs:collapseInto(dest, opts) -> nil
vfs:extractFile(vfsPath, destDir) -> string | nil
vfs:diffDirectory(dir) -> table

vfs:providerRecordsFor(path) -> { ProviderRecord }
vfs:providersFor(path) -> { VfsProvider } | nil
vfs:explain(path) -> table | nil
vfs:duplicates(pattern?) -> table
vfs:archives() -> { ArchiveInfo }
vfs:archiveEntries(archive) -> { ArchiveEntry }
vfs:filesFromArchive(archive) -> { string }
vfs:sourceContributions() -> table
vfs:materializationPlan(dest, opts) -> table
vfs:layerIndex() -> LayerIndex
```

`collapseInto` and `materializationPlan` options:

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
file:path() -> string
file:fileName() -> string | nil
file:fileStem() -> string | nil
file:parentArchivePath() -> string | nil
file:parentArchiveName() -> string | nil
file:readAll() -> string
```

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

layer:keys() -> { string }               -- sorted
layer:sources() -> { { path = string, kind = string } }
layer:sourceIdForPath(path) -> integer | nil
layer:sourceById(id) -> table | nil
layer:sourcesContaining(path) -> { integer }
layer:providerOriginalPath(sourceIndex, path) -> string | nil
layer:providerChain(path) -> { LayerProvider }
layer:duplicateKeys() -> { string }
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

## `ConflictIndex`

```lua
conflicts = vfstool.ConflictIndex.fromDirectories({ dir1, dir2 })
conflicts = vfstool.ConflictIndex.fromFileLists({
  { source = "base", files = { "a.txt" } },
})
conflicts = vfstool.ConflictIndex.fromLayerIndex(layer)

conflicts:sources() -> { string }
conflicts:sourcesContaining(path) -> { integer }
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
