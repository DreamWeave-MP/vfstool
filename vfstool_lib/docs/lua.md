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

Enable it with:

```toml
vfstool_lib = { version = "1", features = ["lua"] }
```

`lua-vendored` is an alias for hosts that want to spell out the vendored Lua choice. The binding is
embedded either way. There is no dynamic Lua module pretending to be a stable C ABI. Good.

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
- semantic `analyze_pair`

It deliberately does **not** bind:

- `experimental::*`
- solver, policy, or knowledge-base helpers
- low-level archive internals
- Rust iterators or parallel iterators

Reports are plain Lua tables. Long-lived mutable structures are userdata.

## Conventions

- Paths are strings.
- Enums are snake-case strings, for example `"loose_dir"`, `"archive"`, `"ini"`,
  `"cosmetic_only"`, `"winner_hash_changed"`.
- Constructors that take directories expect arrays: `{ "/data/base", "/data/mod" }`.
- Archive loading still depends on `beth-archives` / `zip` Cargo features.
- `VFS` stores provider stacks low priority to high priority and caches the resolved winner map.
- Winner-only and stack-preserving mutation use different names. If a method says it removes only the
  winner, it reveals the next provider; if it says it removes the resolved file, it discards the stack.

## Top-level functions

```lua
vfstool.normalize_host_path(path) -> string
vfstool.normalize_host_path_in_place(path) -> string
vfstool.path_glob_matches(glob, path) -> boolean
vfstool.source_glob_matches(glob, source_path) -> boolean

vfstool.analyze_pair(path, left_bytes, right_bytes) -> {
  asset_class = string,
  delta = { kind = string, change_summary = { string }? },
}
```

Run workflow:

```lua
count, snapshot = vfstool.run_setup(vfs, merged_dir, use_hardlinks)
count, metadata_snapshot = vfstool.run_setup_tracked(vfs, merged_dir, use_hardlinks)
snapshot = vfstool.snapshot_directory(dir)
metadata_snapshot = vfstool.snapshot_directory_metadata(dir)
changed = vfstool.changed_files(dir, snapshot)
changed = vfstool.changed_files_metadata(dir, metadata_snapshot)
copied = vfstool.run_finalize(merged_dir, output_dir, snapshot)
copied = vfstool.run_finalize_tracked(merged_dir, output_dir, metadata_snapshot)
```

`run_setup` may hardlink loose files into the merged directory. Child tools that edit files in place
can mutate the original source files through those hardlinks. Use `false` for `use_hardlinks` if the
child tool is not hardlink-safe. This warning is part of the API, not decorative prose.

With `serialize` enabled:

```lua
vfstool.serialize(value, "json" | "yaml" | "toml") -> string
```

## `VFS`

```lua
vfs = vfstool.VFS.new()
vfs = vfstool.VFS.from_directories({ dir1, dir2 }, { archives = { "base.bsa" } })
vfs, conflicts = vfstool.VFS.from_directories_with_conflict_index({ dir1, dir2 })
vfs, layer = vfstool.VFS.from_directories_with_layer_index({ dir1, dir2 })

vfs:len() -> integer
vfs:is_empty() -> boolean
vfs:keys() -> { string }
vfs:entries() -> { { key = string, file = VfsFile } }
vfs:get_file(path) -> VfsFile | nil
vfs:contains(path) -> boolean
vfs:find_by_regex(pattern, relative?) -> table
vfs:remaining(filter_path, replacements_only, all_dirs, relative?) -> table
vfs:paths_matching(substring) -> { { key = string, file = VfsFile } }
vfs:paths_with(prefix) -> { { key = string, file = VfsFile } }

vfs:set_winner_file(key, file) -> VfsFile | nil
vfs:set_winner_loose_file(key, physical_path) -> VfsFile | nil
vfs:push_directory(path) -> nil
vfs:push_archive(path) -> boolean
vfs:push_provider(key, provider) -> boolean
vfs:remove_winner(key) -> VfsProvider | nil
vfs:remove_resolved_file(key) -> VfsFile | nil
vfs:remove_provider(key, source_path) -> { VfsProvider }
vfs:remove_source(source_path) -> { { key = string, provider = table } }
vfs:remove_provider_prefix(prefix) -> { { key = string, provider = table } }
vfs:remove_resolved_prefix(prefix) -> { { key = string, file = VfsFile } }
vfs:remove_resolved_matching_glob(glob) -> { { key = string, file = VfsFile } }

vfs:tree(relative?) -> table
vfs:display(relative?) -> string
vfs:dump_to_directory(dir, use_hardlinks) -> integer
vfs:collapse_into(dest, opts) -> nil
vfs:extract_file(vfs_path, dest_dir) -> string | nil
vfs:diff_directory(dir) -> table

vfs:provider_records_for(path) -> { ProviderRecord }
vfs:providers_for(path) -> { VfsProvider } | nil
vfs:explain(path) -> table | nil
vfs:duplicates(pattern?) -> table
vfs:archives() -> { ArchiveInfo }
vfs:archive_entries(archive) -> { ArchiveEntry }
vfs:files_from_archive(archive) -> { string }
vfs:case_collisions() -> table
vfs:source_contributions() -> table
vfs:validate() -> table
vfs:materialization_plan(dest, opts) -> table
vfs:layer_index() -> LayerIndex
```

`collapse_into` and `materialization_plan` options:

```lua
{
  allow_copying = false,
  extract_archives = false,
  use_symlinks = false,
}
```

## `VfsFile`

```lua
file = vfstool.VfsFile.from(path)
file:is_loose() -> boolean
file:is_archive() -> boolean
file:path() -> string
file:file_name() -> string | nil
file:file_stem() -> string | nil
file:parent_archive_path() -> string | nil
file:parent_archive_name() -> string | nil
file:read_all() -> string
```

## `VfsProvider`

```lua
provider = vfstool.VfsProvider.new({ path = dir, kind = "loose_dir" }, file)
provider:source() -> { path = string, kind = string }
provider:file() -> VfsFile
```

## `LayerIndex`

```lua
layer = vfstool.LayerIndex.from_file_lists({
  { source = { path = "base", kind = "loose_dir" }, files = { "a.txt" } },
})

layer:keys() -> { string }
layer:sources() -> { { path = string, kind = string } }
layer:source_id_for_path(path) -> integer | nil
layer:source_by_id(id) -> table | nil
layer:sources_containing(path) -> { integer }
layer:provider_original_path(source_index, path) -> string | nil
layer:provider_chain(path) -> { LayerProvider }
layer:duplicate_keys() -> { string }
layer:source_contributions() -> table
layer:provenance(vfs, path, with_hashes) -> table | nil
lock = layer:lock_manifest(vfs)
layer:diff_against_lock(vfs, lock) -> table
layer:semantic_conflicts(vfs, opts?) -> table
```

Semantic conflict options:

```lua
{
  archive_hash_mode = "disabled" | "winner_only" | "all_providers",
  include_semantic_deltas = false,
}
```

## `ConflictIndex`

```lua
conflicts = vfstool.ConflictIndex.from_directories({ dir1, dir2 })
conflicts = vfstool.ConflictIndex.from_file_lists({
  { source = "base", files = { "a.txt" } },
})
conflicts = vfstool.ConflictIndex.from_layer_index(layer)

conflicts:sources() -> { string }
conflicts:sources_containing(path) -> { integer }
conflicts:conflicts_report(relative?) -> table
conflicts:shadowed_report(relative?, list_files?) -> table
conflicts:diff_report(source_a, source_b) -> table
```

## Report tables

Report table field names mirror the Rust report structs using snake_case. Nested provider records
use this shape:

```lua
{
  source_index = 1,
  source = { path = "/mods/foo", kind = "loose_dir" },
  key = "textures/foo.dds",
  original_path = "Textures/Foo.DDS",
  resolved_path = "/mods/foo/Textures/Foo.DDS",
}
```

The Lua binding is intentionally boring: deterministic tables in, deterministic tables out. Clever
Lua magic belongs in the host application, not in the FFI seam.
