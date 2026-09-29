# vfstool_lib

OpenMW's virtual file system as a Rust library.

Give it data directories in load order, and archives if you have them, and it builds the VFS
OpenMW would: one normalized key per file, later directories over earlier ones, loose files over
archives. It keeps every provider of every key, not only the winner, so it can explain why a file
resolves the way it does, report conflicts and fully shadowed sources, lock the result with BLAKE3
hashes and report drift, write the VFS out as one folder, and run a tool against it. It does not
depend on OpenMW. [vfstool](https://github.com/DreamWeave-MP/vfstool) is its command line.

**Documentation, including the full Rust and Luau API reference:
<https://dreamweave-mp.github.io/vfstool/>**

## Install

```toml
[dependencies]
vfstool_lib = { version = "1.0", features = ["beth-archives", "zip", "serialize"] }
```

| Feature | Adds |
|---|---|
| `beth-archives` | BSA and BA2 archives, through dream_archive |
| `zip` | ZIP, PK3 and JPK archives, Zstandard entries included |
| `serialize` | JSON, YAML and TOML, and the `serde`, `serde_json`, `serde_yaml` and `toml` re-exports |
| `lua` | The `@dream/vfs` [l3i](https://github.com/DreamWeave-MP/l3i) extension, `lua::VfsExtension` |

None is on by default; without any, the library reads loose files only.

## Usage

```rust
use vfstool_lib::VFS;

// Later directories win; with an archive feature, archives sit below every loose file.
let vfs = VFS::from_directories(["Data Files", "mods/Patch"], Some(vec!["Morrowind.bsa"]));

if let Some(report) = vfs.explain(r"Textures\Tx_Wood_01.dds") {
    println!("{} wins over {} others", report.winner.source.path.display(), report.overridden.len());
}
```

Keys are byte-first `NormalizedPath`s, taken through `VfsKeyInput`: any spelling of a path inside
the VFS works. Paths on disk stay `Path` and `PathBuf`.

## Where to read next

- [Rust API](https://dreamweave-mp.github.io/vfstool/docs/api/): every public type and function
- [How files resolve](https://dreamweave-mp.github.io/vfstool/docs/resolution/): the rules the
  library implements
- [Luau API](https://dreamweave-mp.github.io/vfstool/docs/luau/) and
  [Embedding Luau](https://dreamweave-mp.github.io/vfstool/docs/luau-hosts/)
- [Changelog](https://dreamweave-mp.github.io/vfstool/lib/changelog/)

## MSRV and license

Rust 1.88. Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.
