+++
title = "VfsExtension"
description = "The Rust side of @dream/vfs: the l3i extension a host composes, its constants, the handle types that carry a VFS and its indexes across, and the views a host can push itself."
weight = 70

[extra]
kind = "api"
+++

Rust, in `vfstool_lib::lua`, behind the `lua` feature. [Embedding Luau](@/docs/luau-hosts.md)
walks through a host.

## VfsExtension

{{ api_signature(value="struct VfsExtension") }}

The `dream.vfs` extension: an `l3i::extension::Extension` that describes the `@dream/vfs` module,
its userdata types and views, and their Luau signatures. It holds no state; `Clone`, `Copy`,
`Debug`, `Default`. Add it to a plan with `RuntimePlan::builder().extension(VfsExtension)`, next to
dream-path's `dream_path::lua::PathExtension`, which it requires: a plan without `dream.path` does
not finalize, and the error says `requires 'dream.path'`.

When a runtime is made from the plan, the extension fills in the module's constructor tables,
`VFS`, `VfsFile`, `VfsProvider`, `LayerIndex` and `ConflictIndex`, and freezes them. It never
creates a runtime, takes no capabilities, and installs no global.

The plan gives a tag to the types scripts touch most, so that calls on them dispatch without a
metatable lookup:

| Type | Tagged |
|---|---|
| `dream.vfs.VFS`, `dream.vfs.VfsFile` | Yes, when the plan has tags to spare |
| `dream.vfs.Keys`, `dream.vfs.Entries` | Yes, when the plan has tags to spare |
| `dream.vfs.VfsProvider`, `dream.vfs.LayerIndex`, `dream.vfs.ConflictIndex`, `dream.vfs.VfsLock`, `dream.vfs.Snapshot`, `dream.vfs.MetadataSnapshot`, `dream.vfs.Providers`, `dream.vfs.ProviderRecords`, `dream.vfs.Tree` | Never |

## Constants

{{ api_signature(value='const EXTENSION_ID: &str = "dream.vfs"') }}

The extension's id in the plan.

{{ api_signature(value='const MODULE: &str = "@dream/vfs"') }}

The path scripts `require`.

{{ api_signature(value='const MODULE_NAME: &str = "vfstool"') }}

The conventional global, for a host that wants one:
`RuntimePolicy::new().compat_global(MODULE, MODULE_NAME)`. The extension never sets it.

## Vfs

{{ api_signature(value="struct Vfs") }}

A `VFS` as scripts see it, userdata `dream.vfs.VFS`. It holds the VFS behind a `RefCell`, because
script methods change it through a shared handle, and the sorted key list `keys()` last built,
dropped at the next change.

{{ api_signature(value="fn new(vfs: VFS) -> Vfs") }}

Wraps a VFS the host built.

{{ api_signature(value="fn push(scope: &impl Scope, vfs: VFS) -> Result<ValueView<'_>>") }}

Pushes a new `dream.vfs.VFS` holding `vfs` onto a frame, ready to set as a global or pass to a
function. The runtime must come from a plan with `VfsExtension`; otherwise the type is not
registered and this fails.

{{ api_signature(value="fn with<R>(&self, body: impl FnOnce(&VFS) -> R) -> Result<R>") }}

Runs `body` with the VFS borrowed. Fails with `dream.vfs: the VFS is being mutated` while a
`with_mut` on the same handle is running.

{{ api_signature(value="fn with_mut<R>(&self, body: impl FnOnce(&mut VFS) -> R) -> Result<R>") }}

Runs `body` with the VFS borrowed mutably, and drops the cached key list. Fails with
`dream.vfs: the VFS is in use` while any other borrow is running.

{{ api_signature(value="fn take(&self) -> Result<VFS>") }}

Takes the VFS out and leaves an empty one behind: how a host reclaims a VFS a script built or
changed.

These borrows can only overlap when host code runs Luau from inside `with` or `with_mut`; the
module's own methods never do.

```rust
use l3i::Runtime;
use l3i::extension::{RuntimePlan, RuntimePolicy};
use vfstool_lib::VFS;
use vfstool_lib::lua::{Vfs, VfsExtension};

fn main() -> l3i::Result<()> {
    let plan = RuntimePlan::builder()
        .policy(RuntimePolicy::new().debug_root("host"))
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()?;
    let runtime = Runtime::from_plan(&plan)?;

    // A host function that takes the script's VFS.
    let count = runtime.bind_function("host.countUnder", |vfs: &Vfs, prefix: &str| {
        vfs.with(|vfs| vfs.paths_with(prefix).count() as f64)
    })?;
    runtime.set_global("countUnder", &count)?;

    runtime.exec(r#"
        local vfstool = require("@dream/vfs")
        built = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
        assert(countUnder(built, "textures") == 3)
    "#)?;

    // The script's VFS, taken back.
    let value = runtime.global("built")?;
    let stack = runtime.stack();
    let frame = stack.frame();
    let handle = l3i::userdata::check_receiver::<Vfs>(value.push_to(&frame)?)?;
    let vfs: VFS = handle.take()?;
    assert_eq!(vfs.len(), 5);
    assert!(handle.with(VFS::is_empty)?);
    Ok(())
}
```

## Other handles

Each wraps a `vfstool_lib` value in a public field, so a host can push its own with
`l3i::userdata::push_owned(scope, handle)`, and read one it is handed with
`l3i::userdata::receiver`.

| Handle | Userdata | Wraps | Derives |
|---|---|---|---|
| `VfsFileHandle` | `dream.vfs.VfsFile` | `pub VfsFile` | `Debug`, `Clone` |
| `VfsProviderHandle` | `dream.vfs.VfsProvider` | `pub VfsProvider` | `Debug`, `Clone` |
| `LayerIndexHandle` | `dream.vfs.LayerIndex` | `pub LayerIndex` | `Debug`, `Clone` |
| `ConflictIndexHandle` | `dream.vfs.ConflictIndex` | `pub ConflictIndex` | none |
| `VfsLockHandle` | `dream.vfs.VfsLock` | `pub VfsLock` | `Debug`, `Clone` |
| `SnapshotHandle` | `dream.vfs.Snapshot` | `pub Snapshot` | `Debug` |
| `MetadataSnapshotHandle` | `dream.vfs.MetadataSnapshot` | `pub MetadataSnapshot` | `Debug` |

A `VfsLockHandle` is how a script gets a lock saved earlier: scripts can only make one from a
live VFS, but the host can deserialize a `VfsLock` from the command line's `lock` output and push
it.

## Views

The [sequence views](@/docs/luau/views.md) are `l3i::sequence::SequenceSource` types, pushed with
`l3i::sequence::Sequence::push(scope, view)`; the tree is a `StreamSource`, pushed with
`l3i::sequence::Stream::push(scope, walk)`.

{{ api_signature(value="struct Keys(pub KeyBlob)") }}

`dream.vfs.Keys`, items `string`. `Clone`, `Debug`, `Default`. A host fills one with the
`KeyBlob` below.

{{ api_signature(value="struct KeyBlob") }}

The packed key list inside a `Keys`: one allocation for every key's bytes and one end offset per
key, shared behind an `Rc`. `Clone`, `Debug`, `Default`.

{{ api_signature(value="fn new<'a>(keys: impl IntoIterator<Item = &'a [u8]>) -> KeyBlob") }}

Packs `keys` in the order given, bytes as they are. Panics if they come to 4 GiB or more together.

{{ api_signature(value="fn sorted(keys: impl IntoIterator<Item = NormalizedPath>) -> KeyBlob") }}

Sorts `keys` by their bytes and packs them, as `vfs:keys()` does.

`len`, `is_empty`, and `get(index)`, 0-based, give the count and each key's bytes.

{{ api_signature(value="struct Entries") }}

`dream.vfs.Entries`, items `{ key, file }`. `Clone`, `Debug`, `Default`.

{{ api_signature(value="fn new(entries: impl IntoIterator<Item = (NormalizedPath, VfsFile)>) -> Entries") }}

The entries in the order given.

{{ api_signature(value="fn sorted(entries: impl IntoIterator<Item = (NormalizedPath, VfsFile)>) -> Entries") }}

The entries sorted by key.

{{ api_signature(value="fn cloned<'a>(entries: impl IntoIterator<Item = (&'a NormalizedPath, &'a VfsFile)>) -> Entries") }}

Borrowed pairs, such as `vfs.iter()` or `vfs.paths_with("textures")` yields, cloned and sorted by
key.

{{ api_signature(value="struct Providers(pub Rc<[VfsProvider]>)") }}

`dream.vfs.Providers`, items `{ source, file }`. `Clone`, `Debug`.

{{ api_signature(value="struct ProviderRecords(pub Rc<[VfsProviderRecord]>)") }}

`dream.vfs.ProviderRecords`, items provider records. `Clone`, `Debug`.

{{ api_signature(value="struct TreeWalk") }}

`dream.vfs.Tree`: a `DisplayTree` flattened into rows for iteration, the tree kept for
`toTable()`.

{{ api_signature(value="fn new(tree: DisplayTree) -> TreeWalk") }}

{{ api_signature(value="fn tree(&self) -> &DisplayTree") }}

The tree the rows were flattened from.

```rust
use l3i::Runtime;
use l3i::extension::RuntimePlan;
use l3i::sequence::{Sequence, Stream};
use vfstool_lib::VFS;
use vfstool_lib::lua::{Entries, TreeWalk, VfsExtension};

fn main() -> l3i::Result<()> {
    let plan = RuntimePlan::builder()
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()?;
    let runtime = Runtime::from_plan(&plan)?;
    let vfs = VFS::from_directories(["Data Files", "mods/Door Fix"], None);

    {
        let stack = runtime.stack();
        let frame = stack.frame();
        Sequence::push(&frame, Entries::cloned(vfs.paths_with("meshes")))?;
        frame.set_global("meshes")?;
        Stream::push(&frame, TreeWalk::new(vfs.tree(true)))?;
        frame.set_global("tree")?;
    }
    runtime.exec(r#"
        assert(#meshes == 1 and meshes[1].file:readAll() == "fixed door")
        assert(tree:count() == 4)
    "#)
}
```
