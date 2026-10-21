+++
title = "vfstool_lib"
description = "OpenMW's virtual file system as a Jess package in Luau: provider stacks, conflict and provenance reports, locks and materialization."

[taxonomies]
tags = ["OpenMW", "Jess", "Luau", "VFS"]

[extra]
sections = ["overview", "releases", "credits"]
+++

vfstool_lib is the virtual file system [vfstool](@/home/index.md) runs on, as a crate. Give it
data directories in load order, and archives if you have them, and it builds OpenMW's VFS: one
normalized key per file, later directories over earlier ones, loose files over archives. It keeps
every provider of every key, not only the winner, so it can say why a file resolves the way it
does, what a mod overrides, and what would change if the load order did.

```lua
local vfs = require '@vfs'

-- Two data directories, the second winning: OpenMW's order.
local stack = vfs.VFS.fromDirectories { base, patch }

assert(stack:getFile [[Textures\TX_WOOD_01.DDS]])        -- any spelling of the path
local report = stack:explain 'textures/tx_wood_01.dds'
print(report.key, #report.overridden)                    -- the winner, and every copy it hides
```

- **Lookups** by any spelling of a path, prefix and substring searches, and trees for display or
  serialization.
- **Provider reports**: the chain behind one key, every duplicate, what each archive and data
  directory contributes.
- **Load-order analysis**: conflicts, fully shadowed sources, diffs between sources, and whether
  two versions of an INI, TOML or JSON file differ in what they say or only in how.
- **Locks and drift**: a deterministic manifest of every winner, with BLAKE3 hashes, and a report
  of what changed since.
- **Materialization**: the VFS as one folder, with hardlinks, symbolic links, copies or extracted
  archives, and a plan of what that would do.

BSA and BA2 archives are read through [dream_archive](https://dreamweave-mp.github.io/dream_archive/),
the package `org.dreamweave.dream-archive`.

## Documentation

The package's reference is `vfs/src/init.jsl` and the modules beside it: every function is
documented where it is written, and `jess luau-types` prints the module's types. The guide that
explained the Rust crate lives with it on the `rust` branch; the Luau package's own guide is being
written. [vfstool](@/home/index.md) is the command line over this library.
