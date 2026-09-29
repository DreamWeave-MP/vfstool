+++
title = "Collapsing and extracting"
description = "Write the whole VFS out as one folder, with hardlinks, symbolic links, copies or extracted archives; preview it first; or copy one file out."
weight = 45

[extra]
kind = "guide"
+++

## Collapse

`collapse` writes every file of the VFS into one folder, at its key: the winning copy of each,
under the lowercase path OpenMW asks for. Point a tool, or a game with a simpler data model, at
that folder and it sees what OpenMW sees.

```sh
$ vfstool -c openmw collapse /tmp/mw/merged
vfstool: skipping icons/m/tx_lantern.dds, loaded from archive: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
vfstool: skipping meshes/tr/tr_lantern.nif, loaded from archive: /tmp/mw/Morrowind/Data Files/Tribunal.bsa
```

By default each loose file is a hardlink: no space used, and nothing copied, but it only works on
the same drive as the source, and the linked file is the source file. The archives themselves
are linked like any file; the files that win from inside them are skipped, with the message
above.

| Option | Changes |
|---|---|
| `-s`, `--symbolic` | Symbolic links instead of hardlinks. They work across drives; Windows needs Developer Mode or an administrator to create them |
| `-a`, `--allow-copying` | Where a link cannot be made, copy the file instead of stopping |
| `-e`, `--extract-archives` | Write the winning files from inside archives, and leave out every `.bsa` and `.ba2` file itself |

With `-e` the folder is complete without any archive, and OpenMW, or anything else, can use it
with no `fallback-archive=` lines. It takes the space of everything extracted:

```sh
$ vfstool -c openmw collapse -e /tmp/mw/merged
vfstool: skipping archive Tribunal.bsa
vfstool: skipping archive Morrowind.bsa
```

```text
merged/icons/m/tx_lantern.dds
merged/lantern glow.esp
merged/meshes/l/light_com_lantern_01.nif
merged/meshes/tr/tr_lantern.nif
merged/meshes/xbase_anim.nif
merged/morrowind.esm
merged/textures/tx_lantern_glow.dds
merged/textures/tx_stone_01.dds
merged/textures/tx_wood_01.dds
merged/tribunal.esm
```

`-e` leaves out every file named `.bsa` or `.ba2`, loaded or not, except
`ArchiveInvalidationInvalidated!.bsa`.

The folder is created if it is missing. A file already at a destination is replaced, unless it
already is the file that belongs there: collapsing into one of the cfg's data directories leaves
that directory's own files where they are. A folder at a destination, or a symbolic link on the
way to it, stops the collapse. A link that fails without `-a`, or an archive entry that cannot be
read, stops it too, with exit code 9, and leaves what was written so far. Files are written in
parallel, so the messages come in no fixed order.

{% callout(kind="warning", title="Collapsing into a data directory changes that mod") %}
Every file the VFS takes from elsewhere is added to the folder, and with `-e` every file from
inside the archives, and OpenMW sees them there from then on. That is what unpacking a game's
archives into its own `Data` folder wants; for anything else, collapse into a new, empty folder.
{% end %}

{% callout(kind="warning", title="Links are the mod's own files") %}
Editing a linked file in the collapsed folder in place, hardlink or symbolic link, edits the file
in the mod it came from. `collapse` has no option that copies every file; for a tool that writes,
use [`run --copy`](@/docs/run.md), which does.
{% end %}

## Other games

Nothing in a cfg has to be Morrowind's. A cfg that lists another game's data folders with
`data=` and its BSAs or BA2s with `fallback-archive=`, Oblivion's or Skyrim's, gives vfstool that
game's VFS, with the same rules, and `collapse -a -e` into a new folder writes it out with every
archive unpacked.

## Preview it

`--dry-run` prints what `collapse` would do, and writes nothing:

```sh
$ vfstool -c openmw collapse --dry-run -e /tmp/mw/merged
actions:
- !extract_archive
  key: icons/m/tx_lantern.dds
  archive: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
  dest: /tmp/mw/merged/icons/m/tx_lantern.dds
- !hardlink
  key: lantern glow.esp
  source: /tmp/mw/mods/Lantern Glow/Lantern Glow.esp
  dest: /tmp/mw/merged/lantern glow.esp
- !skip_archive_file
  key: morrowind.bsa
  archive: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
```

That is three of its actions; the plan has one per file, sorted by key, then an `issues` list:
loose files that no longer exist, and destinations that cannot be written safely. The plan names
a `hardlink` or `symlink` for every loose file, including one already in place, which `collapse`
leaves alone; it cannot know in advance which links will fail, so `-a` does not change it. `-f` picks YAML, JSON or TOML, and `-o` writes it to a file.
[Report formats](@/docs/formats.md#collapse-dry-run) lists every action.

## Extract one file

`extract` copies one file out of the VFS into a folder, from a data directory or from inside an
archive:

```sh
$ vfstool -c openmw extract icons/m/tx_lantern.dds /tmp/mw/out
[ SUCCESS ]: Successfully extracted icons/m/tx_lantern.dds to /tmp/mw/out
```

The file lands directly in the folder, under the last part of its key: `/tmp/mw/out/tx_lantern.dds`.
The folder is created if it is missing, and a file of the same name is replaced. It is always a
copy, except that a loose file extracted into the folder it already lives in stays as it is. A
path that is not in the VFS exits with 1:

```sh
$ vfstool -c openmw extract meshes/nothing.nif /tmp/mw/out
[ ERROR ]: Couldn't locate meshes/nothing.nif in the vfs!
```
