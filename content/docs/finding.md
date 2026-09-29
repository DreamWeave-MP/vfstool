+++
title = "Finding files"
description = "Where one file comes from, every file matching a pattern, the whole chain behind a file, every file with more than one copy, and what a mod still provides."
weight = 30

[extra]
kind = "guide"
+++

All of these read the VFS and write nothing. The output below is from the
[example install](@/docs/start-here.md#the-example-install).

## One file

`find-file` takes a path inside the VFS, in any case and with either slash, and prints the file
OpenMW would load for it:

```sh
$ vfstool -c openmw find-file textures/tx_wood_01.dds
[ SUCCESS ]: Successfully found VFS File textures/tx_wood_01.dds at path /tmp/mw/mods/Crisp Textures/Textures/Tx_Wood_01.dds
```

`-s` prints only the path, without colors, for scripts. A file inside an archive is printed as
the archive's path followed by the path you asked for.

```sh
$ vfstool -c openmw find-file -s 'Textures\TX_STONE_01.dds'
/tmp/mw/mods/Crisp Textures/Textures/Tx_Stone_01.dds
```

A file that is not there exits with 1. With `-p` (`--only_physical`), a file that only an
archive provides counts as not found, and exits with 2:

```sh
$ vfstool -c openmw find-file -p meshes/xbase_anim.nif
[ ERROR ]: Failed to locate meshes/xbase_anim.nif in loose files of the provided VFS.
```

`find-file` looks through the data directories first, highest priority first, and only builds
the whole VFS, archives included, when no loose file matches. A lookup that a loose file answers
is fast on any size of install.

## Every file matching a pattern

`find` matches a regular expression against every key, ignoring case, and prints the matches as
a tree of folders:

```sh
$ vfstool -c openmw find lantern
/:
  tmp:
    mw:
      Morrowind:
        Data Files:
          Morrowind.bsa:
            icons:
              m:
                .:
                - icons\m\tx_lantern.dds
          Tribunal.bsa:
            meshes:
              tr:
                .:
                - meshes\tr\tr_lantern.nif
      mods:
        Crisp Textures:
          Textures:
            .:
            - Tx_Lantern_Glow.dds
        Lantern Glow:
          .:
          - Lantern Glow.esp
          Meshes:
            L:
              .:
              - Light_Com_Lantern_01.NIF
```

Only winners are listed, each where it really is: loose files under their data directory, archive
entries under their archive. A folder's own files are under the key `.`.

The expression is matched against keys, which are lowercase with `/`, and is used as written:
`\.nif$` is every file ending in `.nif`, and a backslash is a regular expression escape, not a
separator. It is Rust's [regex](https://docs.rs/regex) syntax; an expression that does not
compile exits with 6. The tree is YAML unless `-f` asks for `json` or `toml`, and `-o` writes it
to a file:

```sh
$ vfstool -c openmw find -f json '\.nif$'
{"/":{"tmp":{"mw":{"Morrowind":{"Data Files":{"Morrowind.bsa":{"meshes":{".":["meshes\\xbase_anim.nif"]}},"Tribunal.bsa":{"meshes":{"tr":{".":["meshes\\tr\\tr_lantern.nif"]}}}}},"mods":{"Lantern Glow":{"Meshes":{"L":{".":["Light_Com_Lantern_01.NIF"]}}}}}}}}
```

With `-r` (`--use-relative`, before the command), the tree starts at a folder named `Data Files`
and follows keys instead of the folders on disk, with each archive's files under its name:

```sh
$ vfstool -c openmw -r find lantern
Data Files:
  .:
  - Lantern Glow.esp
  Morrowind.bsa:
    icons:
      m:
        .:
        - icons\m\tx_lantern.dds
  Tribunal.bsa:
    meshes:
      tr:
        .:
        - meshes\tr\tr_lantern.nif
  meshes:
    l:
      .:
      - Light_Com_Lantern_01.NIF
  textures:
    .:
    - Tx_Lantern_Glow.dds
```

On Linux and macOS, an archive entry is listed by its whole path inside the archive, with the
archive's backslashes, as above; on Windows, by its file name.

## Explain one file

`explain` shows every provider of a key, the winner and then the rest from lowest priority to
highest. [Start here](@/docs/start-here.md#see-why) shows it for `tx_wood_01.dds`, and
[Report formats](@/docs/formats.md#provider-records-explain-duplicates) lists its fields. A key nobody provides exits with 1.

## Every file with more than one copy

`duplicates` lists every key with more than one provider, the same records as `explain`, and
`winner_index`, the position of the winner in `providers`. An optional regular expression, matched
against keys like `find`'s, narrows the list:

```sh
$ vfstool -c openmw duplicates '^textures/tx_stone'
entries:
- key: textures/tx_stone_01.dds
  providers:
  - source_index: 0
    source:
      path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
      kind: archive
    key: textures/tx_stone_01.dds
    original_path: textures\tx_stone_01.dds
    resolved_path: /tmp/mw/Morrowind/Data Files/Morrowind.bsa::textures\tx_stone_01.dds
  - source_index: 2
    source:
      path: /tmp/mw/Morrowind/Data Files
      kind: loose_dir
    key: textures/tx_stone_01.dds
    original_path: Textures/Tx_Stone_01.DDS
    resolved_path: /tmp/mw/Morrowind/Data Files/Textures/Tx_Stone_01.DDS
  - source_index: 5
    source:
      path: /tmp/mw/mods/Crisp Textures
      kind: loose_dir
    key: textures/tx_stone_01.dds
    original_path: Textures/Tx_Stone_01.dds
    resolved_path: /tmp/mw/mods/Crisp Textures/Textures/Tx_Stone_01.dds
  winner_index: 2
```

## What a mod still provides

`remaining` takes one data directory, spelled as its `data=` line spells it, and prints the files
it still provides: the ones where it is the winner.

```sh
$ vfstool -c openmw -r remaining '/tmp/mw/mods/Lantern Glow'
Data Files:
  .:
  - Lantern Glow.esp
  meshes:
    l:
      .:
      - Light_Com_Lantern_01.NIF
```

With its own `-r` (`--replacements-only`, after the command), it prints the opposite: files the
directory has but a later one provides instead. The global `-r` goes before the command:

```sh
$ vfstool -c openmw -r remaining -r '/tmp/mw/mods/Lantern Glow'
Data Files:
  textures:
    .:
    - Tx_Lantern_Glow.dds
    - Tx_Wood_01.dds
```

A folder that is not one of the cfg's data directories exits with 8. Matching ignores ASCII case
and the direction of slashes.
