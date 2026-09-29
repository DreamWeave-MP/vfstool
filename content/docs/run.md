+++
title = "Running tools on the merged VFS"
description = "Give a tool that expects one data folder the merged VFS, then keep only what it created or changed, in data-local or a folder you name."
weight = 50

[extra]
kind = "guide"
+++

Many modding tools expect one data folder: a plugin converter, a texture optimizer, a navmesh
generator. `run` gives them the merged VFS as that folder, the way Mod Organizer 2 does for
Bethesda games, and keeps what they wrote as a new layer of your load order, without touching
the mods.

```sh
vfstool run [OPTIONS] <MERGED_DIR> -- <COMMAND>...
```

## What it does

1. Writes every file of the VFS into `MERGED_DIR`, at its key: loose files hardlinked (or copied,
   with `--copy` or when the folder is on another drive), and the winning files inside archives
   extracted. The archive files themselves are written too.
2. Hashes every file it wrote.
3. Runs the command. Every argument that is exactly `{}` is replaced by `MERGED_DIR`.
4. If the command succeeds, hashes the folder again and copies every file that is new, or whose
   content changed, into `data-local`, or into `--output`, at the same relative path. Each copy is
   printed as `relative path -> destination`.
5. Deletes `MERGED_DIR`, unless `--keep-merged` is given.

```sh
$ vfstool -c openmw run --copy /tmp/mw/merged -- sh -c 'echo remade > "$1/textures/tx_stone_01.dds"; echo new > "$1/textures/tx_new.dds"' sh {}
Dumping VFS to /tmp/mw/merged...
Dumped 12 files.
Capturing 2 changed file(s) to /tmp/mw/openmw/data...
textures/tx_new.dds -> /tmp/mw/openmw/data/textures/tx_new.dds
textures/tx_stone_01.dds -> /tmp/mw/openmw/data/textures/tx_stone_01.dds
```

`data-local` is the last data directory OpenMW reads, so what the tool wrote now wins over every
mod, and the mods themselves are as they were. Removing the tool's work is deleting those files.

## Options

| Option | Does |
|---|---|
| `--copy` | Copy loose files into the folder instead of hardlinking them |
| `--output <DIR>` | Where changed files go, instead of `data-local`. Needed when the cfg has no `data-local` |
| `--keep-merged` | Keep the merged folder afterwards |
| `--working-dir <DIR>` | The command's working directory. The default is vfstool's |

{% callout(kind="danger", title="Without --copy, the tool edits your mods") %}
A hardlink is the mod's file under a second name. A tool that rewrites a file in place, instead of
writing a new one, rewrites the original in the mod folder, and the changed copy is then captured
into `data-local` too. Use `--copy` for any tool you are not sure about; it costs the space and
time of copying the loose files once.
{% end %}

## The command

Everything after `--` is the command and its arguments. It runs directly, not through a shell:
for pipes, globs or `$1`, run a shell yourself, as the example does. `{}` is replaced only when it
is a whole argument; `--data={}` is passed as it is.

## When things go wrong

| Situation | Result |
|---|---|
| `MERGED_DIR` exists and is not empty | Exit 8 before anything is written, and the folder is left alone |
| No `data-local` in the cfg and no `--output` | Exit 8 before anything is written |
| The command exits with a nonzero code | Nothing is captured; vfstool exits with the same code |
| The command cannot be started | Exit 9 |
| The command is ended by a signal | Nothing is captured; exit 9 |

The merged folder is deleted in each case once it exists, unless `--keep-merged` is given.

```sh
$ vfstool -c openmw run /tmp/mw/merged -- sh -c 'exit 3'
Dumping VFS to /tmp/mw/merged...
Dumped 13 files.
vfstool: subprocess exited with exit status: 3, not capturing files.
```

## What is not captured

Only new and changed files. A file the tool deletes from the merged folder is not deleted
anywhere else, and a file it only renames is captured under the new name while the old one stays
in its mod. Everything runs on files: `run` does not change `openmw.cfg`.
