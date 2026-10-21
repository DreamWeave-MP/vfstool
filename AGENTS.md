# Agent guidance for vfstool

Two Luau packages, the workspace members `vfs/` (the library, `org.dreamweave.vfs`) and `vfstool/`
(the command line, `org.dreamweave.vfstool`), each a Package Source with its own `jess.toml`, `src`,
`tests` and `bench`. The website (`config.toml`, `content/`, `sass/brand.sass`, `static/img`,
`static/js`) has a project per package: `content/lib` is the library, `content/home` the tool. It is
built by the DreamWeave Mod Template through jess (`jess-workspace.toml` names the template;
`jess.lock` pins it). The Rust program and crate live on the `rust` branch and are not developed here.

## Verification

```sh
jess workspace sync --locked
jess workspace test
jess workspace run --trusted dreamweave-check
```

Each project's `mod.toml` says `format = "jpk"` and names its package with `jess`; its `[[releases]]`
are oldest first, each version matching the member's `jess.toml` at its tag, `vfstool-<version>` or
`vfstool_lib-<version>`. Never edit a `mod.lock`; CI writes them.
