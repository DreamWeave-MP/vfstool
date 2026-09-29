+++
title = "Embedding Luau"
description = "Give scripts @dream/vfs through l3i: the features, the toolchain, composing the extensions, handing a script a VFS and taking it back, the vfstool global, type definitions, and moving from the 0.10 binding."
weight = 75

[extra]
kind = "guide"
+++

With the `lua` feature, vfstool_lib is an [l3i](https://github.com/DreamWeave-MP/l3i) extension: it
describes the `@dream/vfs` module, and a Rust host that runs Luau through l3i composes it into its
runtime. Scripts then build, query, change and write out virtual file systems with the same code
as the command line. The crate never creates a VM and never installs a global; both are the host's
decisions. The [Luau API](@/docs/luau/_index.md) lists what scripts get.

## Dependencies and toolchain

```toml
[dependencies]
vfstool_lib = { version = "1.0", features = ["lua", "beth-archives", "zip", "serialize"] }
dream-path = { version = "1.0", features = ["lua"] }
l3i = "1.0"
```

`lua` is the binding. The others are optional and add to it: `beth-archives` reads BSA and BA2
archives and `zip` ZIP archives, the renamed PK3 and JPK included, and either one adds `vfs:pushArchive`; `serialize` adds
`serialize` and `vfs:serializeTree`. dream-path is a direct dependency because the host adds its
`PathExtension` to the plan.

l3i builds Luau itself, and only with clang, lld and cross-language thin LTO: its build script
refuses any other configuration and names the missing piece. Cargo does not pass a dependency's
configuration on, so the host copies the policy into its own `.cargo/config.toml`, as vfstool
does:

```toml
[env]
CXX = "clang++"
CXX_x86_64-pc-windows-msvc = "clang-cl"
CXX_aarch64-pc-windows-msvc = "clang-cl"

[target.x86_64-unknown-linux-gnu]
rustflags = ["-Clinker-plugin-lto", "-Clinker=clang", "-Clink-arg=-fuse-ld=lld"]

[target.aarch64-unknown-linux-gnu]
rustflags = ["-Clinker-plugin-lto", "-Clinker=clang", "-Clink-arg=-fuse-ld=lld"]

[target.x86_64-apple-darwin]
rustflags = ["-Clinker=clang", "-Clink-arg=-fuse-ld=lld"]

[target.aarch64-apple-darwin]
rustflags = ["-Clinker=clang", "-Clink-arg=-fuse-ld=lld"]

[target.x86_64-pc-windows-msvc]
rustflags = ["-Clinker-plugin-lto", "-Clinker=lld-link"]

[target.aarch64-pc-windows-msvc]
rustflags = ["-Clinker-plugin-lto", "-Clinker=lld-link"]
```

clang and rustc must use the same LLVM major version. Apple targets link with clang and lld but
leave out `-Clinker-plugin-lto`, whose arguments Apple's lld rejects. The [l3i toolchain
notes](https://github.com/DreamWeave-MP/l3i/blob/main/TOOLCHAIN.md) have the measurements behind
the rule.

## Composing the extensions

`VfsExtension` goes into the host's `RuntimePlan` together with dream-path's `PathExtension`, which
it requires, next to whatever else the host provides. Every runtime made from the plan can
`require("@dream/vfs")`:

```rust
use l3i::Runtime;
use l3i::extension::RuntimePlan;
use vfstool_lib::lua::VfsExtension;

fn main() -> l3i::Result<()> {
    let plan = RuntimePlan::builder()
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()?;
    let runtime = Runtime::from_plan(&plan)?;

    runtime.exec(r#"
        local vfstool = require("@dream/vfs")
        local vfs = vfstool.VFS.fromDirectories({ "Data Files", "mods/Wood Retexture" })
        assert(vfs:getFile("textures/tx_wood_01.dds"):readAll() == "new wood")
    "#)
}
```

The examples on this page run from the folder that holds the [example
install](@/docs/luau/module.md#the-example-install).

## Handing scripts a VFS

A host that already built its VFS, from the user's `openmw.cfg` for instance, pushes it as
userdata with `vfstool_lib::lua::Vfs::push` and sets it where scripts will find it. When the
script is done, the host takes it back, with whatever the script changed:

```rust
use l3i::Runtime;
use l3i::extension::RuntimePlan;
use vfstool_lib::VFS;
use vfstool_lib::lua::{Vfs, VfsExtension};

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
        Vfs::push(&frame, vfs)?;
        frame.set_global("install")?;
    }

    runtime.exec(r#"
        install:removeResolvedMatchingGlob("**/*.zip")
        assert(install:len() == 3)
    "#)?;

    let value = runtime.global("install")?;
    let stack = runtime.stack();
    let frame = stack.frame();
    let vfs = l3i::userdata::check_receiver::<Vfs>(value.push_to(&frame)?)?.take()?;
    assert_eq!(vfs.len(), 3);
    Ok(())
}
```

`take` leaves an empty VFS in the script's handle. To look without taking, use `with`; a host
function that takes a `&Vfs` argument gets the script's VFS the same way. The
[extension page](@/docs/luau/extension.md) has both, and the other handle types.

## The vfstool global

The 0.10 binding installed a `vfstool` global. A host that still wants one exposes the module as a
compatibility global through its policy; the global and `require` then return the same table:

```rust
use l3i::Runtime;
use l3i::extension::{RuntimePlan, RuntimePolicy};
use vfstool_lib::lua::{MODULE, MODULE_NAME, VfsExtension};

fn main() -> l3i::Result<()> {
    let policy = RuntimePolicy::new().compat_global(MODULE, MODULE_NAME);
    let plan = RuntimePlan::builder()
        .policy(policy)
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()?;
    let runtime = Runtime::from_plan(&plan)?;

    runtime.exec(r#"assert(vfstool == require("@dream/vfs"))"#)
}
```

`MODULE` is `"@dream/vfs"` and `MODULE_NAME` is `"vfstool"`.

## Types for editors and checks

Every function and method carries a Luau signature, and every view declares its item type.
`plan.type_definitions()` returns the `.d.luau` text for everything in the plan, `@dream/vfs`
included, ready to save for an editor's language server:

```rust
use l3i::extension::RuntimePlan;
use vfstool_lib::lua::VfsExtension;

fn main() -> l3i::Result<()> {
    let plan = RuntimePlan::builder()
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()?;
    let definitions = plan.type_definitions();
    assert!(definitions.contains("declare extern type dream_vfs_VFS with"));
    std::fs::write("dream.d.luau", definitions).expect("write the definitions");
    Ok(())
}
```

With l3i's `analysis` feature, `plan.check_definitions()` type-checks those definitions.
vfstool_lib's tests run it under the `luau-analysis` feature (`cargo test -p vfstool_lib --features
luau-analysis`), and type-check a strict script that measures, indexes and iterates views, reads
files into buffers, and takes a lock, so the declared API and the runtime cannot drift apart.
[Type definitions](@/docs/luau/types.md) shows what the plan generates.

## What scripts get

- **The disk, read and write.** Scripts can read any file a VFS or `VfsFile.from` points at, and
  write wherever the process can: `collapseInto`, `dumpToDirectory`, `extractFile` and
  `runFinalize` create files, and `runSetup` deletes its target directory first, whatever it
  holds. A host that runs scripts it does not trust must not give them this module.
- **Handles for the big things, tables for the rest.** The VFS, files, providers, indexes, locks
  and snapshots are userdata; lists are views over native memory; reports are plain tables.
- **Bytes in, bytes out.** Paths are byte strings, with no UTF-8 requirement, and keys are
  normalized once per call without allocating.
- **Strict options.** A misspelled or snake_case option is an error that lists the right names.
- **No threads.** Every call runs on the calling thread and returns when it is done. Building a
  VFS, walking a directory or hashing files uses Rust's worker threads inside the call.

## From 0.10

`lua::open(&Lua)` and `lua::register(&Lua)` are gone, with `mlua`. Compose `VfsExtension` and
`PathExtension` as above; `RuntimePolicy::compat_global("@dream/vfs", "vfstool")` brings back the
global. The functions and methods have the same names. What changed:

- `keys`, `entries`, `pathsMatching`, `pathsWith`, `providersFor`, `providerRecordsFor`,
  `filesFromArchive`, `layer:keys` and `layer:duplicateKeys` return [sequence
  views](@/docs/luau/views.md). `#`, `[i]` and `for` work as before; `ipairs`, `pairs` and the
  `table` library need `:toTable()`.
- `tree`, `findByRegex` and `remaining` return a [Tree](@/docs/luau/views.md#tree) stream;
  `tree:toTable()` is the nested table they returned.
- Indices count from 1: `sourceIndex`, `providerIndex` and `winnerIndex` in reports, the results of
  `sourcesContaining` and `sourceIdForPath`, and the arguments of `sourceById` and
  `providerOriginalPath`. They counted from 0.
- `size` and `winnerSize` are Luau integers: compare with `4i`, or convert with `tonumber`.
- Option tables are strict: an unknown key in `{ archives = ... }`, the collapse options, the
  semantic options or a source table is an error instead of being ignored.
- Paths are bytes: a key that is not UTF-8 is no longer a conversion error, and `file:path()`
  returns the exact bytes rather than a lossy string.
- `vfs:len()` counts in constant time instead of walking every key.
- New: `file:readInto(buffer, offset?)`, `tree:count()`, and `len()` on snapshots. `lock:toTable()`
  has the shape it had.
