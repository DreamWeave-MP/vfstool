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
creates a runtime, requires no capability, and installs no global.

### Capabilities

The extension declares two optional capabilities, one per write feature, and reads them at
install, so one compiled crate serves a trusted tool and a sandboxed session from the same plan
shape:

| Constant | Capability | Feature | Grants |
|---|---|---|---|
| `WRITE_CAPABILITY` | `filesystem.write` | `lua-write` | The `writeRoot` constructor option and `vfs:setWriteRoot`, so a VFS a script builds can have a write root at all |
| `HOST_CAPABILITY` | `filesystem.host` | `lua-host` | The functions of `vfstool.host` |

A host grants them on the policy: `RuntimePolicy::new().capability(WRITE_CAPABILITY)`. Without
the grant the module keeps its shape (the type definitions do not change), and the gated members
raise a permission error naming the capability: `dream.vfs: setWriteRoot requires the
'filesystem.write' capability, which this runtime does not grant`, `dream.vfs: host.readFile
requires the 'filesystem.host' capability, which this runtime does not grant`. The `VFS`
constructors are installed per runtime and carry its grant into every VFS they make; a `Vfs` the
host makes in Rust carries the host's own grant instead (below). [Writing files and host
I/O](@/docs/luau/io.md#capabilities) has the script's view.

The plan gives a tag to the types scripts touch most, so that calls on them dispatch without a
metatable lookup:

| Type | Tagged |
|---|---|
| `dream.vfs.VFS`, `dream.vfs.VfsFile`, `dream.vfs.Reader` | Yes, when the plan has tags to spare |
| `dream.vfs.Keys`, `dream.vfs.Entries` | Yes, when the plan has tags to spare |
| `dream.vfs.VfsProvider`, `dream.vfs.LayerIndex`, `dream.vfs.ConflictIndex`, `dream.vfs.VfsLock`, `dream.vfs.Snapshot`, `dream.vfs.MetadataSnapshot`, `dream.vfs.Providers`, `dream.vfs.ProviderRecords`, `dream.vfs.Tree`, `dream.vfs.Writer`, `dream.vfs.HostEntries` | Never |

With the `lua-write` feature the extension adds the write methods to `dream.vfs.VFS` and the
`writeRoot` constructor option; with `lua-host` it fills the module's `host` table at install,
with the functions or with stubs as the runtime's policy says. [Writing files and host
I/O](@/docs/luau/io.md) describes both. It never installs a `require`
navigator; [require over the VFS](@/docs/luau/require.md) is the host's call.

## Constants

{{ api_signature(value='const EXTENSION_ID: &str = "dream.vfs"') }}

The extension's id in the plan.

{{ api_signature(value='const MODULE: &str = "@dream/vfs"') }}

The path scripts `require`.

{{ api_signature(value='const MODULE_NAME: &str = "vfstool"') }}

The conventional global, for a host that wants one:
`RuntimePolicy::new().compat_global(MODULE, MODULE_NAME)`. The extension never sets it.

{{ api_signature(value='const WRITE_CAPABILITY: &str = "filesystem.write"') }}

{{ api_signature(value='const HOST_CAPABILITY: &str = "filesystem.host"') }}

The [capabilities](#capabilities) the policy grants with `RuntimePolicy::capability`.

## Vfs

{{ api_signature(value="struct Vfs") }}

A `VFS` as scripts see it, userdata `dream.vfs.VFS`. It holds the VFS behind a `RefCell`, because
script methods change it through a shared handle, the sorted key list `keys()` last built,
dropped at the next change, and with `lua-write` the write root.

The handle is shared: it is `Clone`, and a clone is the same VFS. A host keeps one clone for a
[`VfsRequireNavigator`](@/docs/luau/require.md) or a callback and pushes another to scripts with
`l3i::userdata::push_owned(scope, handle.clone())`; a change a script makes is visible through
both, and a writer a script opened registers its file through the same VFS on `close`.

{{ api_signature(value="fn new(vfs: VFS) -> Vfs") }}

Wraps a VFS the host built.

{{ api_signature(value="fn shares(&self, other: &Vfs) -> bool") }}

Whether `other` is a clone of this handle.

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

{{ api_signature(value="fn write_root(&self) -> Option<PathBuf>") }}

{{ api_signature(value="fn set_write_root(&self, root: Option<PathBuf>) -> io::Result<()>") }}

With `lua-write`: the directory writes go under, and how a host grants or withdraws it. The
directory is created if absent and kept in canonical form, which is what `write_root` returns;
the error is the creation's or canonicalization's, and leaves the root as it was. A root the host
sets works in any runtime, whatever its capabilities: the host is trusted.

{{ api_signature(value="fn write_root_grant(&self) -> WriteRootGrant") }}

{{ api_signature(value="fn set_write_root_grant(&self, grant: WriteRootGrant)") }}

With `lua-write`: who may give this VFS its write root from a script, an enum of three:

| `WriteRootGrant` | Meaning | Set by |
|---|---|---|
| `Scripts` | `setWriteRoot` and the `writeRoot` option work | The constructors, in a runtime that grants `filesystem.write`; a host that wants scripts to move the root of a VFS it pushed |
| `Refused` | Both raise the capability error, and the VFS never has a root | The constructors, in a runtime without the grant |
| `Host` | `setWriteRoot` raises `dream.vfs: setWriteRoot: the host made this VFS in Rust and keeps its write root; only the host sets it` | `Vfs::new` |

A `Vfs` the host pushes therefore never hands scripts a root they did not have, in any runtime,
unless the host says `Scripts`.

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
| `VfsFileHandle` | `dream.vfs.VfsFile` | `pub file: VfsFile`, made with `VfsFileHandle::new(file)`, plus the backing its positional reads keep | `Debug`, `Clone` |
| `VfsReader` | `dream.vfs.Reader` | A position over a file's backing; only `file:open()` and `host.open` make one | `Debug` |
| `VfsWriter` | `dream.vfs.Writer` | A buffered file writer (`lua-write` or `lua-host`); only `vfs:openWrite` and `host.openWrite` make one | `Debug` |
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

{{ api_signature(value="struct HostEntries(pub Rc<[HostEntry]>)") }}

With `lua-host`: `dream.vfs.HostEntries`, items `{ path, isDir, size }`, the rows of
`host.list`; `HostEntry { path: PathBuf, is_dir: bool, size: u64 }`. `Clone`, `Debug`.

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
