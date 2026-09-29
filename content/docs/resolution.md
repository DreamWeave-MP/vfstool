+++
title = "How files resolve"
description = "Which directories and archives vfstool reads from openmw.cfg, how a path becomes a key, which copy of a file wins, and what is left out."
weight = 20

[extra]
kind = "guide"
+++

vfstool builds the VFS the way OpenMW does, from the same lines of `openmw.cfg`. The program and
`vfstool_lib` share this code, so everything here holds for both.

## What it reads

| cfg line | Becomes |
|---|---|
| `data=` | A data directory: every file under it, in any subfolder, symbolic links followed |
| `resources=` | `<resources>/vfs`, a data directory before every `data=` line |
| `data-local=` | A data directory after every `data=` line. If the folder does not exist, reading the cfg creates it |
| `fallback-archive=` | An archive, found by name among the files of the data directories |
| `config=` | Another cfg, read in turn, as OpenMW reads it |

`content=` and `groundcover=` do not change the VFS; [`validate`](@/docs/checking.md) checks
that each one is in it.

## Keys

Every file gets a key: its path inside its data directory or archive, with `\` turned into `/`,
ASCII letters lowercased, repeated separators collapsed and leading ones removed.
`Textures\Tx_Wood_01.DDS` and `textures/tx_wood_01.dds` are the same key, and so the same file.

Only ASCII is folded. Every other byte is kept as it is, so `Ä` and `ä` are different keys, and a
name that is not UTF-8 is a key like any other. Wherever vfstool takes a path inside the VFS, on
the command line or in the library, it normalizes it the same way first.

## Which copy wins

Several sources can have the same key. Each one is a provider, and vfstool keeps all of them,
lowest priority first. The last is the winner, the copy OpenMW loads:

1. **Archives, in the order of the `fallback-archive=` lines.** A later archive overrides an
   earlier one.
2. **Data directories, in the order of the `data=` lines**, with `resources` first and
   `data-local` last. A later directory overrides an earlier one.

Every loose file outranks every archive, wherever the archive sits. A texture in `Morrowind.bsa`
loses to the same texture in the first `data=` directory. The reports number the sources in this
order: in the [example install](@/docs/start-here.md#the-example-install), `Morrowind.bsa` is 0,
`Tribunal.bsa` 1, `Data Files` 2, and `data-local` 6.

## Archives

An archive named by `fallback-archive=` is looked up like any file: its name is a key, and the
winning copy is the one opened. `fallback-archive=Morrowind.bsa` finds `Morrowind.bsa` at the top
of whichever data directory has it; if two do, the later one's is used.

The program reads PC BSA and BA2 archives through
[dream_archive](https://crates.io/crates/dream_archive): Morrowind's BSAs, the BSAs of Oblivion,
Fallout 3, New Vegas and Skyrim, and the BA2s of Fallout 4 and later. The format is read from the
file, not its extension. Console layouts are not read. ZIP and PK3 archives are read by
`vfstool_lib` with its `zip` feature, which the released program is not built with.

An archive that is named but not found, or found but unreadable, is left out, and the VFS is
built without it. `validate` lists the first kind, and `validate --full` both.

The archive file itself is also a file in the VFS: `morrowind.bsa` is a key, provided by
`Data Files`. `collapse` and `run` copy or link it like any other file, unless they extract its
contents instead.

## What is left out

The VFS is always one that can be written out as a folder. While it is built, these are skipped:

- files and folders that cannot be read;
- names that would leave the folder: an absolute path, a `..` component, a drive letter;
- a key that would need a file and a folder at the same path, such as a file `meshes/l` when
  `meshes/l/light.nif` is already there. The key that arrives second is dropped: archives arrive
  first, then the data directories in order.

Nothing is reported while building. `validate` checks the cfg's own lines; the provider reports
([`explain`](@/docs/finding.md#explain-one-file), [`duplicates`](@/docs/finding.md#every-file-with-more-than-one-copy))
show what was accepted.
