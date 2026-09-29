# vfstool

OpenMW's virtual file system, from the command line.

OpenMW stacks every `data=` directory in `openmw.cfg`, and every `fallback-archive=`, into one
virtual file system: the last directory with a file wins, and any loose file beats any archive.
vfstool builds the same VFS from the same cfg and answers questions about it: where a file comes
from, what it overrides, which mods are hidden entirely, what changed since last week. It can
write the whole VFS out as one folder, or hand that folder to a tool and keep what the tool wrote.

The VFS itself is `vfstool_lib`, a Rust library with an l3i Luau module, `@dream/vfs`, in this
workspace.

**Documentation, downloads and the full Rust and Luau API reference:
<https://dreamweave-mp.github.io/vfstool/>**

## Install

Download the build for your system from the
[releases](https://github.com/DreamWeave-MP/vfstool/releases): Windows, macOS on Apple silicon and
Intel, and Linux. Or build it:

```sh
cargo install vfstool
```

## Use

vfstool reads `openmw.cfg` from OpenMW's default folder, or from the folder `--config` names:

```sh
vfstool find-file textures/tx_wood_01.dds      # the copy OpenMW loads
vfstool explain textures/tx_wood_01.dds        # and every copy it hides
vfstool conflicts                              # which sources override which
vfstool validate                               # missing directories, archives and plugins
vfstool lock -o vfs-lock.yaml                  # every winner, hashed
vfstool drift --fail-on-drift vfs-lock.yaml    # what changed since
vfstool collapse -e /path/to/new/folder        # the whole VFS as one folder
```

## As a library

```toml
[dependencies]
vfstool_lib = { version = "1.0", features = ["beth-archives", "serialize"] }
```

```rust
use vfstool_lib::VFS;

let vfs = VFS::from_directories(["Data Files", "mods/Patch"], Some(vec!["Morrowind.bsa"]));
let wood = vfs.get_file(r"Textures\Tx_Wood_01.dds");
```

With the `lua` feature, `vfstool_lib::lua::VfsExtension` is an
[l3i](https://github.com/DreamWeave-MP/l3i) extension providing the module `@dream/vfs`: the VFS,
its files with positional reads and a sequential reader, indexes, views and reports. `lua-write`
adds writes under a VFS write root, which contains keys lexically and does not follow symbolic
links, and `lua-host` a `host` table over any host path; both are opt-in features, and each
runtime gets them only when its policy grants the `filesystem.write` or `filesystem.host`
capability;
`VfsRequireNavigator` gives scripts `require` over the VFS with OpenMW's script template cache.

## Where to read next

- [Start here](https://dreamweave-mp.github.io/vfstool/docs/start-here/): a first look at your
  load order
- [How files resolve](https://dreamweave-mp.github.io/vfstool/docs/resolution/): keys, priority
  and archives
- [Command line](https://dreamweave-mp.github.io/vfstool/docs/cli/): every command, option and
  exit code
- [Rust API](https://dreamweave-mp.github.io/vfstool/docs/api/) and
  [Luau API](https://dreamweave-mp.github.io/vfstool/docs/luau/)

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -D warnings
cargo test --workspace --all-features
```

The library's `lua` feature needs l3i's toolchain, clang, lld and cross-language thin LTO, which
`.cargo/config.toml` sets for the whole workspace. The site in `content/` is a
[DreamWeave Mod Template](https://github.com/DreamWeave-MP/DreamWeave-Mod-Template) site; preview it
with `zola serve`.

## License

vfstool and vfstool_lib are licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
