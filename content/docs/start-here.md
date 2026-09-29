+++
title = "Start here"
description = "Download vfstool, point it at your openmw.cfg, and ask it whether the load order is complete, where a file comes from, and why."
weight = 10

[extra]
kind = "guide"
+++

## Get it

Download the archive for your system from the [project page](@/home/index.md), unzip it
anywhere, and run `vfstool` from a terminal. There is nothing to install and no window: every
command prints to the terminal, or writes a file you name.

With Rust installed, `cargo install vfstool` builds the same program. On Arch Linux, the AUR
package `vfstool-git` builds it from the repository.

## Point it at your config

vfstool reads the same `openmw.cfg` OpenMW does. With no options it uses OpenMW's default
location:

| System | Folder |
|---|---|
| Windows | `Documents\My Games\openmw` |
| macOS | `~/Library/Preferences/openmw` |
| Linux | `$XDG_CONFIG_HOME/openmw`, or `~/.config/openmw` |

For any other install, name the folder that holds `openmw.cfg` with `--config`, before the
command:

```sh
vfstool --config "D:\Games\OpenMW" validate
```

`--config` takes the folder, not the file. A cfg with another name goes in `OPENMW_CONFIG`, as
the full path to the file. [Choosing the config](@/docs/cli.md#choosing-the-config) has the
rest, including Flatpak installs.

## The example install

The output on these pages comes from a small install built for them in `/tmp/mw`: Morrowind's
`Data Files` with two BSAs, and three mods. Every command ran in `/tmp/mw` with `-c openmw`. Its
`openmw/openmw.cfg`:

```ini
data="/tmp/mw/Morrowind/Data Files"
data="/tmp/mw/mods/Old Wood"
data="/tmp/mw/mods/Lantern Glow"
data="/tmp/mw/mods/Crisp Textures"
data-local="/tmp/mw/openmw/data"
fallback-archive=Morrowind.bsa
fallback-archive=Tribunal.bsa
content=Morrowind.esm
content=Tribunal.esm
content=Lantern Glow.esp
```

## Check the load order

`validate` checks that every `data=` folder exists, and that every archive, content file and
groundcover file the cfg names is in one of them:

```sh
$ vfstool -c openmw validate
issues: []
```

An empty list is a complete load order. Otherwise each problem is listed by name and vfstool
exits with 5. [Validating and locking](@/docs/checking.md) shows each kind.

## Find a file

Ask for a file by its path inside the VFS, as the game does. Case and the direction of the
slashes do not matter:

```sh
$ vfstool -c openmw find-file textures/tx_wood_01.dds
[ SUCCESS ]: Successfully found VFS File textures/tx_wood_01.dds at path /tmp/mw/mods/Crisp Textures/Textures/Tx_Wood_01.dds
```

That is the copy OpenMW loads. A file only an archive provides is shown inside the archive:

```sh
$ vfstool -c openmw find-file meshes/xbase_anim.nif
[ SUCCESS ]: Successfully found VFS File meshes/xbase_anim.nif at path /tmp/mw/Morrowind/Data Files/Morrowind.bsa/meshes/xbase_anim.nif
```

`-s` prints the path alone, for scripts.

## See why

Four other sources have `tx_wood_01.dds`. `explain` shows the winner and every copy it hides,
lowest priority first:

```sh
$ vfstool -c openmw explain 'Textures\TX_WOOD_01.dds'
key: textures/tx_wood_01.dds
winner:
  source_index: 5
  source:
    path: /tmp/mw/mods/Crisp Textures
    kind: loose_dir
  key: textures/tx_wood_01.dds
  original_path: Textures/Tx_Wood_01.dds
  resolved_path: /tmp/mw/mods/Crisp Textures/Textures/Tx_Wood_01.dds
overridden:
- source_index: 0
  source:
    path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
    kind: archive
  key: textures/tx_wood_01.dds
  original_path: textures\tx_wood_01.dds
  resolved_path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa::textures\tx_wood_01.dds
- source_index: 1
  source:
    path: /tmp/mw/Morrowind/Data Files/Tribunal.bsa
    kind: archive
  key: textures/tx_wood_01.dds
  original_path: textures\tx_wood_01.dds
  resolved_path: /tmp/mw/Morrowind/Data Files/Tribunal.bsa::textures\tx_wood_01.dds
- source_index: 3
  source:
    path: /tmp/mw/mods/Old Wood
    kind: loose_dir
  key: textures/tx_wood_01.dds
  original_path: Textures/Tx_Wood_01.dds
  resolved_path: /tmp/mw/mods/Old Wood/Textures/Tx_Wood_01.dds
- source_index: 4
  source:
    path: /tmp/mw/mods/Lantern Glow
    kind: loose_dir
  key: textures/tx_wood_01.dds
  original_path: Textures/Tx_Wood_01.dds
  resolved_path: /tmp/mw/mods/Lantern Glow/Textures/Tx_Wood_01.dds
```

The archives come first because every loose file outranks every archive, whatever the order of
the `data=` lines. [How files resolve](@/docs/resolution.md) has the whole order.

## Next

- **[Finding files](@/docs/finding.md)**: searching the whole VFS, and what a mod still provides.
- **[Conflicts and load order](@/docs/conflicts.md)**: the same questions for whole mods.
- **[Collapsing and extracting](@/docs/collapse.md)**: writing the VFS out.
- **[Command line](@/docs/cli.md)**: every command and option.
