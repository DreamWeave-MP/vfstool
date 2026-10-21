# vfstool

OpenMW's virtual file system as two Jess packages in Luau, developed together in this repository:

| Folder | Package | Is |
|---|---|---|
| `vfs/` | `org.dreamweave.vfs`, module `@vfs` | The library: provider stacks, lookups, conflict and provenance reports, locks, materialization |
| `vfstool/` | `org.dreamweave.vfstool`, command `vfstool` | The command line over it: find, explain, conflicts, lock, drift, collapse, run |

```toml
[dependencies]
"org.dreamweave.vfs" = "2"

[dev-dependencies]
"org.dreamweave.vfstool" = "2"
```

The site at <https://dreamweave-mp.github.io/vfstool/> is this repository, built by the DreamWeave
Mod Template through jess on every push; each project's releases are JPKs on this repository's
GitHub Releases, tagged `vfstool-<version>` and `vfstool_lib-<version>`, and `jess workspace sync`
takes them from there.

The Rust program and crate, `vfstool` 1.0.1 and `vfstool_lib` 1.0.0 and below, live on the `rust`
branch and stay on crates.io and their GitHub releases. They are no longer developed here.

## Developing

`jess workspace test` runs both packages' suites. `jess workspace run --trusted dreamweave-check`
validates the site; `dreamweave-site -- public <url>` renders it.
