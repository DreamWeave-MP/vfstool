# vfstool_lib

`vfstool_lib` is a reimplementation of OpenMW's virtual file system (VFS). It provides tools for working with directory structures, archive files, and file metadata — ideal for applications that handle complex mod loadouts. It does not depend on OpenMW or any particular game.

---

## Features

- **Virtual File System (VFS)**: Build from ordered data directories. Later directories win (matching OpenMW `data=` semantics). Loose files always beat archive files.
- **Conflict analysis**: Per-source override and overridden-by sets, plus high-level reports for the `conflicts`, `shadowed`, `which`, `stats`, and `diff` queries.
- **Archive support**: BSA/BA2 (Morrowind, Oblivion, Skyrim, Fallout 4) via the `ba2` crate (`bsa` feature). ZIP/PK3 via the `zip` crate (`zip` feature).
- **Serialization**: JSON, YAML, TOML output via `serde` (`serialize` feature).
- **Parallel processing**: Directory walks and hash operations use `rayon`.
- **MO2-style runner support**: `run_setup` / `run_finalize` for dump-run-collect workflows.
- **Mutable views**: Winner-only mutation on `VFS`, plus provider-aware mutation with `MutableVfs`.

---

## Installation

```toml
[dependencies]
vfstool_lib = "1.0"
```

With archive and serialization support:

```toml
[dependencies]
vfstool_lib = { version = "1.0", features = ["bsa", "serialize"] }
```

---

## Usage

### Basic example

```rust
use vfstool_lib::vfs::VFS;
use std::path::PathBuf;

fn main() {
    let search_dirs = vec![
        PathBuf::from("path/to/base"),
        PathBuf::from("path/to/mod"),   // higher priority
    ];
    let archive_list = Some(vec!["Morrowind.bsa"]);

    let vfs = VFS::from_directories(search_dirs, archive_list);

    for (key, file) in vfs.iter() {
        println!("{key:?} → {file:?}");
    }
}
```

### Conflict analysis

```rust
use vfstool_lib::vfs::VFS;

let (vfs, ci) = VFS::from_directories_with_conflict_index(
    vec!["path/to/base", "path/to/mod"],
    None,
);

let report = ci.conflicts_report(true);  // use_relative = true
for entry in &report.sources {
    println!("{}: {} overrides, {} overridden",
        entry.path.display(),
        entry.overrides.len(),
        entry.overridden_by.len());
}
```

### Serialization

```rust
use vfstool_lib::{vfs::VFS, SerializeType};

let vfs = VFS::from_directories(vec!["path/to/data"], None);
let tree = vfs.tree(false, None);
let json = vfs.serialize_from_tree(&tree, SerializeType::Json).unwrap();
println!("{json}");
```

### Mutating VFS contents

`VFS` mutators edit the materialized winner map directly. Removing a winner removes that key from
the resolved view; it does not reveal lower-priority providers.

```rust
use vfstool_lib::{VFS, VfsFile};

let mut vfs = VFS::new();
vfs.insert_file("textures/foo.dds", VfsFile::from("/mods/high/textures/foo.dds"));
let removed = vfs.remove_file("Textures/Foo.dds");
assert!(removed.is_some());
```

Use `MutableVfs` when provider stacks matter. Providers are stored low-to-high priority, so removing
the current winner reveals the next lower-priority provider if one exists.

```rust
use vfstool_lib::MutableVfs;

let mut mutable = MutableVfs::from_directories(["/mods/base", "/mods/high"])?;
mutable.remove_winner("textures/foo.dds");
let resolved = mutable.to_vfs();
# Ok::<(), std::io::Error>(())
```

`MutableVfs` source removal uses lexical path equality. Use the same source path representation for
removal that you used when inserting/building providers.

### Runner hardlink behavior

`run_setup` can populate the merged directory with hardlinks. This is intentional for speed and disk
usage, but tools that edit merged files in place may mutate the original loose source files through
those hardlinks. Use copy mode when running tools that are not hardlink-safe.

---

## Feature flags

| Flag | Description |
|------|-------------|
| `bsa` | BSA/BA2 archive support (Morrowind, Oblivion, Skyrim, Fallout 4) |
| `zip` | ZIP/PK3 archive support |
| `serialize` | JSON/YAML/TOML output via serde |

---

## License

Dual-licensed under [MIT](../LICENSE-MIT) or [Apache 2.0](../LICENSE-APACHE) at your option.
