+++
title = "vfstool_lib"
description = "OpenMW's virtual file system as a Rust library: provider stacks, conflict and provenance reports, locks, materialization, and the @dream/vfs Luau module."

[taxonomies]
tags = ["OpenMW", "Rust", "Luau", "VFS"]

[extra]
sections = ["overview", "install", "releases", "credits"]
+++

vfstool_lib is the virtual file system [vfstool](@/home/index.md) runs on, as a crate. Give it
data directories in load order, and archives if you have them, and it builds OpenMW's VFS: one
normalized key per file, later directories over earlier ones, loose files over archives. It keeps
every provider of every key, not only the winner, so it can say why a file resolves the way it
does, what a mod overrides, and what would change if the load order did.

```rust
use std::fs;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-lib-overview");
    let base = root.join("Data Files");
    let patch = root.join("Patch");
    fs::create_dir_all(base.join("Textures"))?;
    fs::create_dir_all(patch.join("textures"))?;
    fs::write(base.join("Textures/Tx_Wood_01.dds"), b"base")?;
    fs::write(patch.join("textures/tx_wood_01.dds"), b"patch")?;

    let vfs = VFS::from_directories([&base, &patch], None);

    let wood = vfs.get_file(r"Textures\TX_WOOD_01.DDS").expect("in the VFS");
    assert_eq!(wood.path(), patch.join("textures/tx_wood_01.dds"));

    let report = vfs.explain("textures/tx_wood_01.dds").expect("in the VFS");
    assert_eq!(report.overridden.len(), 1);

    fs::remove_dir_all(root)
}
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
- **Luau**: the same API as the l3i extension `@dream/vfs`, for hosts that run scripts.

BSA and BA2 archives are behind the `beth-archives` feature, ZIP and PK3 behind `zip`, JSON, YAML
and TOML behind `serialize`, and Luau behind `lua`. With none of them, the crate reads loose files.

## Documentation

- **[Rust API](@/docs/api/_index.md)**: every public type and function, page by page.
- **[Luau API](@/docs/luau/_index.md)** and **[Embedding Luau](@/docs/luau-hosts.md)**: the
  `@dream/vfs` module, and giving it to scripts.
- **[How files resolve](@/docs/resolution.md)**: the rules the library implements, shared with
  the command line.
