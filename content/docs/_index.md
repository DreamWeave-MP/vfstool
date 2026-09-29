+++
title = "Documentation"
description = "How vfstool builds OpenMW's virtual file system from openmw.cfg, every command it has, and the vfstool_lib Rust and Luau API under it."
template = "docs/section.html"
page_template = "docs/page.html"
sort_by = "weight"

[extra]
docs_root = true
docs_project_name = "vfstool"
docs_short_title = "vfstool docs"
docs_project_path = "@/home/index.md"
docs_repository_url = "https://github.com/DreamWeave-MP/vfstool/tree/main/content/docs"
docs_sidebar_label = "Documentation"
hide_child_cards = true
kind = "guide"
+++

vfstool reads an `openmw.cfg`, builds the virtual file system OpenMW would build from it, and
answers questions about it: where a file comes from, what it overrides, what changed since last
week. It can also write that VFS out as one directory. The command line and the library,
`vfstool_lib`, are the same code, so what these pages say about how a file resolves holds for both.

## Learn it

- **[Start here](@/docs/start-here.md)**: download it, point it at your `openmw.cfg`, and ask it
  where a file comes from.
- **[How files resolve](@/docs/resolution.md)**: keys, data directory order, archives, and the
  provider stack behind every file.

## Use it

- **[Finding files](@/docs/finding.md)**: one file, a pattern, the whole chain behind a file, and
  what is left of a mod.
- **[Conflicts and load order](@/docs/conflicts.md)**: which mods override which, what is fully
  hidden, and what each source contributes.
- **[Validating and locking](@/docs/checking.md)**: missing data directories and content files, and
  a lock that tells you when the load order's result changes.
- **[Collapsing and extracting](@/docs/collapse.md)**: the whole VFS as one directory, or one file
  out of it.
- **[Running tools on the merged VFS](@/docs/run.md)**: give a tool that expects one data directory
  the merged tree, and keep what it writes.
- **[Embedding Luau](@/docs/luau-hosts.md)**: giving scripts `@dream/vfs` through l3i.

## Look it up

- **[Command line](@/docs/cli.md)**: every command and option, and the exit codes.
- **[Report formats](@/docs/formats.md)**: the fields of every report the commands write.
- **[Platforms and performance](@/docs/compatibility.md)**: the downloads, features, license,
  what is tested, and what the benchmarks measure.
- **[Rust API](@/docs/api/_index.md)**: `VFS`, its reports, the indexes, and the rest of
  `vfstool_lib`.
- **[Luau API](@/docs/luau/_index.md)**: the `@dream/vfs` module and the extension that provides
  it.
