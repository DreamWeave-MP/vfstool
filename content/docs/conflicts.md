+++
title = "Conflicts and load order"
description = "Which data directories and archives override which, which are hidden entirely, what each one contributes, how two compare, and what each archive provides."
weight = 35

[extra]
kind = "guide"
+++

[Finding files](@/docs/finding.md) asks about one file at a time. These commands ask the same
questions about whole sources: every data directory and every loaded archive, numbered in load
order as [How files resolve](@/docs/resolution.md#which-copy-wins) describes. They read the VFS
and write nothing. The output is from the
[example install](@/docs/start-here.md#the-example-install).

## Who overrides whom

`conflicts` lists every source with two lists of keys: `overrides`, the files it has that a
lower source also has, and `overridden_by`, the files it has that a higher source also has. A
file can be in both: `Old Wood`'s `tx_wood_01.dds` hides the archives' copies and is hidden by
`Lantern Glow`'s.

```sh
$ vfstool -c openmw -r conflicts
sources:
- path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
  overrides: []
  overridden_by:
  - meshes/l/light_com_lantern_01.nif
  - textures/tx_stone_01.dds
  - textures/tx_wood_01.dds
- path: /tmp/mw/Morrowind/Data Files/Tribunal.bsa
  overrides:
  - textures/tx_wood_01.dds
  overridden_by:
  - textures/tx_wood_01.dds
- path: /tmp/mw/Morrowind/Data Files
  overrides:
  - textures/tx_stone_01.dds
  overridden_by:
  - textures/tx_stone_01.dds
- path: /tmp/mw/mods/Old Wood
  overrides:
  - textures/tx_wood_01.dds
  overridden_by:
  - textures/tx_wood_01.dds
- path: /tmp/mw/mods/Lantern Glow
  overrides:
  - meshes/l/light_com_lantern_01.nif
  - textures/tx_wood_01.dds
  overridden_by:
  - textures/tx_lantern_glow.dds
  - textures/tx_wood_01.dds
- path: /tmp/mw/mods/Crisp Textures
  overrides:
  - textures/tx_lantern_glow.dds
  - textures/tx_stone_01.dds
  - textures/tx_wood_01.dds
  overridden_by: []
- path: /tmp/mw/openmw/data
  overrides: []
  overridden_by: []
```

Every source is listed, in load order, an empty data directory too. With the global `-r` (`--use-relative`),
files are keys, as here. Without it, each is the file's path on disk, the source's path joined with
the file's own spelling, `/tmp/mw/mods/Crisp Textures/Textures/Tx_Wood_01.dds`, and an archive's is
the archive and the key with `::` between them:
`/tmp/mw/Morrowind/Data Files/Morrowind.bsa::textures/tx_wood_01.dds`.

## Mods that do nothing

`shadowed` lists the sources whose every file a higher source overrides: a mod you could remove
without changing the game. It prints a count to standard error, and the list to standard output:

```sh
$ vfstool -c openmw shadowed
1 sources are fully shadowed
sources:
- path: /tmp/mw/mods/Old Wood
```

`-l` (`--list-files`) adds each source's files, as `conflicts` spells them.

## What each source contributes

`contributions` counts, for every source, the files it has and what becomes of them:

```sh
$ vfstool -c openmw contributions
sources:
- source_index: 0
  source:
    path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
    kind: archive
  winning_files: 2
  overriding_files: 0
  overridden_files: 3
  unique_files: 2
  duplicate_files: 3
  loose_files: 0
  archive_files: 5
- source_index: 3
  source:
    path: /tmp/mw/mods/Old Wood
    kind: loose_dir
  winning_files: 0
  overriding_files: 1
  overridden_files: 1
  unique_files: 0
  duplicate_files: 1
  loose_files: 1
  archive_files: 0
```

That is two of its seven sources; the empty `data-local` folder is the last, with every count
zero. The counts:

| Field | Counts the source's files that |
|---|---|
| `winning_files` | win: OpenMW loads this source's copy |
| `overriding_files` | a lower source also has |
| `overridden_files` | a higher source also has |
| `unique_files` | no other source has |
| `duplicate_files` | another source also has |
| `loose_files`, `archive_files` | it has, by kind of source; one of the two is always zero |

A source that has files and wins none of them is what `shadowed` lists.

## Two sources side by side

`diff` compares two data directories by their keys, and says which of the two is later in the
load order:

```sh
$ vfstool -c openmw diff '/tmp/mw/mods/Lantern Glow' '/tmp/mw/mods/Crisp Textures'
source_a: /tmp/mw/mods/Lantern Glow
source_b: /tmp/mw/mods/Crisp Textures
higher_priority: /tmp/mw/mods/Crisp Textures
shared:
- textures/tx_lantern_glow.dds
- textures/tx_wood_01.dds
only_in_a:
- lantern glow.esp
- meshes/l/light_com_lantern_01.nif
only_in_b:
- textures/tx_stone_01.dds
```

Both must be data directories of the cfg, or vfstool exits with 8. Case and `\` or `/` do not
matter: each names the `data=` line it matches, and the report spells it as that line does.

## Archives

`archives` lists the archives that were found and opened, with their source index, how many files
each holds, and how many of those win:

```sh
$ vfstool -c openmw archives
- source_index: 0
  path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
  entry_count: 5
  winning_entry_count: 2
- source_index: 1
  path: /tmp/mw/Morrowind/Data Files/Tribunal.bsa
  entry_count: 2
  winning_entry_count: 1
```

`archive-list` lists one archive's files, whether each wins, and its name as the archive stores it:

```sh
$ vfstool -c openmw archive-list Tribunal.bsa
- key: meshes/tr/tr_lantern.nif
  archive_path: /tmp/mw/Morrowind/Data Files/Tribunal.bsa
  original_path: meshes\tr\tr_lantern.nif
  wins: true
- key: textures/tx_wood_01.dds
  archive_path: /tmp/mw/Morrowind/Data Files/Tribunal.bsa
  original_path: textures\tx_wood_01.dds
  wins: false
```

Name the archive by its full path, its file name, or the end of its path, as long as only one
loaded archive matches; case and slashes do not matter. `--source-index` takes the number
`archives` prints instead. A name that matches none, or more than one, exits with 8 and lists
the archives there are:

```sh
$ vfstool -c openmw archive-list Bloodmoon.bsa
[ ERROR ]: No loaded archive matches 'Bloodmoon.bsa'. Available archives: 0=/tmp/mw/Morrowind/Data Files/Morrowind.bsa, 1=/tmp/mw/Morrowind/Data Files/Tribunal.bsa
```

`archives` and `archive-list` write lists. TOML cannot hold one at the top level, so with
`-f toml` the list is under `archives` or `entries`.
