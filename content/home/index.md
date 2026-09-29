+++
title = "vfstool"
description = "OpenMW's virtual file system from the command line: find any file your load order provides, see why it wins, lock the result, and collapse it into one folder."

[taxonomies]
tags = ["OpenMW", "Morrowind", "Rust", "VFS", "Command line"]
+++

OpenMW does not read its files from one `Data Files` folder. It stacks every `data=` directory
in your `openmw.cfg`, and every archive listed as `fallback-archive=`, into one virtual file
system, and asks that for `meshes/xbase_anim.nif`. The last directory that has the file wins, and
any loose file beats any archive. With forty mods installed, which copy of a texture the game
loads has one right answer and no easy way to see it.

vfstool builds the same VFS from the same `openmw.cfg`, and answers:

```sh
$ vfstool find-file -s textures/tx_wood_01.dds
/tmp/mw/mods/Crisp Textures/Textures/Tx_Wood_01.dds
```

`explain` shows every copy behind that one, in load order; `conflicts`, `shadowed` and
`contributions` do the same for whole mods. `lock` writes down what every file resolves to, and
`drift` tells you later what changed. `collapse` writes the whole VFS out as one folder, and `run`
hands that folder to a tool that only understands one, then keeps what the tool wrote.

- **Finding**: `find-file`, `find`, `explain`, `duplicates`, `remaining`, `extract`.
- **Load order**: `conflicts`, `shadowed`, `contributions`, `diff`, `archives`, `archive-list`.
- **Checking**: `validate` for missing directories, archives and content files; `lock` and
  `drift` for changes.
- **Writing**: `collapse`, with links, copies or extracted archives, and a dry run; `run`.

It reads BSA and BA2 archives, from Morrowind's to Fallout 4's, writes its reports as YAML, JSON
or TOML, and changes nothing on disk unless writing is the command's job.

## Documentation

- **[Start here](@/docs/start-here.md)**: download it, point it at your `openmw.cfg`, and ask it
  where a file comes from.
- **[How files resolve](@/docs/resolution.md)**: keys, data directory order, archives, and the
  providers behind each file.
- **[Command line](@/docs/cli.md)**: every command and option, and the exit codes.
- **[vfstool_lib](@/lib/index.md)**: the same VFS as a Rust library and a Luau module.
