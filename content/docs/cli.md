+++
title = "Command line"
description = "Every vfstool command and option, how it finds openmw.cfg, its output formats, and its exit codes, checked against vfstool --help."
weight = 60

[extra]
kind = "reference"
+++

```sh
vfstool [--config <DIR>] [--use-relative] <COMMAND> [OPTIONS]
```

The global options go before the command; `vfstool find-file -c openmw …` is an error.
`vfstool --help` and `vfstool <COMMAND> --help` print the same options as this page. There is no
`--version`.

## Global options

| Option | Does |
|---|---|
| `-c`, `--config <DIR>` | The folder holding `openmw.cfg`. See [Choosing the config](#choosing-the-config) |
| `-r`, `--use-relative` | Write report paths relative: keys instead of paths on disk, in `find`, `remaining`, `conflicts` and `shadowed` |
| `-h`, `--help` | Print help |

## Choosing the config

vfstool loads one `openmw.cfg`, and follows it as OpenMW does. The first of these that applies
picks it:

1. `--config <DIR>`: the file named `openmw.cfg` in that folder, in any case. A folder that does
   not exist, or has no `openmw.cfg`, exits with 7. `--config` names the folder, never the file.
2. `OPENMW_CONFIG`: the full path of a cfg file, of any name. A path that is not a file exits with 7.
3. `openmw.cfg` in OpenMW's default folder: `Documents\My Games\openmw` on Windows,
   `~/Library/Preferences/openmw` on macOS, and `$XDG_CONFIG_HOME/openmw` or `~/.config/openmw` on
   Linux. When vfstool itself runs inside a Flatpak, or `OPENMW_CONFIG_USING_FLATPAK` is set, it is
   `~/.var/app/org.openmw.OpenMW/config/openmw` instead, with `OPENMW_FLATPAK_ID` or `FLATPAK_ID`
   in place of `org.openmw.OpenMW` when set.

```sh
OPENMW_CONFIG=/games/profiles/survival.cfg vfstool validate
```

A cfg that cannot be parsed exits with 7. Loading a cfg creates its `data-local` folder if it is
missing. [How files resolve](@/docs/resolution.md#what-it-reads) lists the lines that build the VFS.

## Output

Reports are YAML unless `-f` (`--format`) says `json` or `toml`. JSON is compact, one line. They
go to standard output, or with `-o` (`--output`) to a file, whose folder is created if needed and
which is replaced if it exists. Messages and errors go to standard error. `find-file`, `extract`
and errors are colored with ANSI codes, whatever the output is; `find-file -s` is plain.

TOML cannot hold a list at the top level, so with `-f toml` the lists `archives` and
`archive-list` write are under a key of their own. [Report formats](@/docs/formats.md) lists every
report's fields.

## Finding

### find-file

```sh
vfstool find-file [-p] [-s] <PATH>
```

Prints where the file OpenMW would load for `PATH` is: a loose file's path, or an archive's path
followed by `PATH`. `PATH` is a path inside the VFS, in any case, with either slash. Exits with 1
if no source has it.

| Option | Does |
|---|---|
| `-p`, `--only_physical` | Match loose files only. A file only an archive has exits with 2 |
| `-s`, `--simple` | Print the path alone, without colors |

### find

```sh
vfstool find [-f FORMAT] [-o FILE] <PATTERN>
```

Prints every winning file whose key matches the regular expression `PATTERN`, ignoring case, as a
folder tree. The expression is matched against keys (`textures/tx_wood_01.dds`) and used as
written. A bad expression exits with 6. [Finding files](@/docs/finding.md#every-file-matching-a-pattern).

### explain

```sh
vfstool explain [-f FORMAT] [-o FILE] <PATH>
```

The winner of `PATH` and every provider it overrides, lowest priority first. Exits with 1 if no
source has it.

### duplicates

```sh
vfstool duplicates [-f FORMAT] [-o FILE] [PATTERN]
```

Every key with more than one provider, with all of them and the winner's position. `PATTERN`, a
regular expression over keys, ignoring case, narrows the list; a bad one exits with 6.

### remaining

```sh
vfstool remaining [-r] [-f FORMAT] [-o FILE] <DATA_DIR>
```

The files `DATA_DIR` still provides, as a tree. `DATA_DIR` must be one of the cfg's data
directories, compared ignoring ASCII case and slash direction, or vfstool exits with 8.

| Option | Does |
|---|---|
| `-r`, `--replacements-only` | Instead, the files `DATA_DIR` has that a later source provides |

This `-r` goes after the command; the global `-r` before it.

## Load order

### conflicts

```sh
vfstool conflicts [-f FORMAT] [-o FILE]
```

Every source with files, in load order, with the files it overrides and the files overridden in
it. [Conflicts and load order](@/docs/conflicts.md#who-overrides-whom).

### shadowed

```sh
vfstool shadowed [-l] [-f FORMAT] [-o FILE]
```

The sources whose every file a higher source overrides. Prints their count to standard error.

| Option | Does |
|---|---|
| `-l`, `--list-files` | List each shadowed source's files |

### contributions

```sh
vfstool contributions [-f FORMAT] [-o FILE]
```

Per source: files that win, override, are overridden, are unique or duplicated, and how many are
loose or archived.

### diff

```sh
vfstool diff [-f FORMAT] [-o FILE] <SOURCE_A> <SOURCE_B>
```

The keys two data directories share and the keys only each has, and which of the two is later.
Both must be data directories of the cfg, in any case, or vfstool exits with 8.

### archives

```sh
vfstool archives [-f FORMAT] [-o FILE]
```

Every archive that was loaded: its source index, path, number of files, and number that win.

### archive-list

```sh
vfstool archive-list [-f FORMAT] [-o FILE] <ARCHIVE>
vfstool archive-list [-f FORMAT] [-o FILE] --source-index <N>
```

One archive's files: key, name in the archive, and whether it wins. `ARCHIVE` is the archive's
full path, file name or path suffix, as long as exactly one loaded archive matches; case and slash
direction do not matter.

| Option | Does |
|---|---|
| `--source-index <N>` | Pick the archive by the `source_index` that `archives` prints, instead of `ARCHIVE` |

No match, several matches, or an unknown index exits with 8 and lists the loaded archives.

## Checking

### validate

```sh
vfstool validate [--full] [-f FORMAT] [-o FILE]
```

Missing `data=` directories, fallback archives, content files and groundcover files. Any issue
exits with 5, after the report is written.

| Option | Does |
|---|---|
| `--full` | Build the whole VFS first: report archives that cannot be opened, and count files inside archives |

### lock

```sh
vfstool lock [-f FORMAT] [-o FILE]
```

A manifest of every file in the VFS: key, winning source and its kind, BLAKE3 hash and size of the
winning copy, and number of providers, sorted by key.

### drift

```sh
vfstool drift [--fail-on-drift] [-f FORMAT] [-o FILE] <LOCK_FILE>
```

Compares the VFS with a lock, read as JSON or TOML by its extension, otherwise YAML. A lock that
cannot be read or parsed, or whose `schema_version` is not 1, exits with 9.

| Option | Does |
|---|---|
| `--fail-on-drift` | Exit with 4 when anything drifted |

## Writing

### collapse

```sh
vfstool collapse [-a] [-e] [-s] [--dry-run [-f FORMAT] [-o FILE]] <DIR>
```

Writes every file of the VFS into `DIR`, at its key. [Collapsing and extracting](@/docs/collapse.md).

| Option | Does |
|---|---|
| `-a`, `--allow-copying` | Copy a file where a link cannot be made |
| `-e`, `--extract-archives` | Extract the winning files from archives, and leave out `.bsa` and `.ba2` files |
| `-s`, `--symbolic` | Symbolic links instead of hardlinks |
| `--dry-run` | Print the plan and write nothing. `-f` and `-o` apply to the plan |

### extract

```sh
vfstool extract <PATH> <DIR>
```

Copies the file OpenMW would load for `PATH` into `DIR`, under the last part of its key. Exits
with 1 if no source has it.

### run

```sh
vfstool run [--copy] [--keep-merged] [--output DIR] [--working-dir DIR] <MERGED_DIR> -- <COMMAND>...
```

Writes the VFS into `MERGED_DIR`, runs `COMMAND` with every `{}` argument replaced by
`MERGED_DIR`, and copies what it created or changed into `data-local` or `--output`.
[Running tools on the merged VFS](@/docs/run.md).

| Option | Does |
|---|---|
| `--copy` | Copy loose files instead of hardlinking them |
| `--keep-merged` | Keep `MERGED_DIR` afterwards |
| `--output <DIR>` | Where captured files go, instead of `data-local` |
| `--working-dir <DIR>` | The command's working directory |

## Exit codes

| Code | Means |
|---|---|
| 0 | Done |
| 1 | `find-file`, `explain` or `extract`: the path is not in the VFS |
| 2 | `find-file -p`: the file is only in an archive. Also a command line that does not parse |
| 4 | `drift --fail-on-drift`: something drifted |
| 5 | `validate`: at least one issue |
| 6 | `find` or `duplicates`: the regular expression does not compile |
| 7 | The config could not be found or loaded |
| 8 | Invalid input: a folder that is not a data directory, an archive selector that matches nothing or too much, a non-empty merged folder, no place to capture to |
| 9 | Anything else that failed: reading, writing, linking, a report that cannot be serialized, a command `run` cannot start |

After its command starts, `run` exits with the command's own exit code, or 9 if it was ended by a
signal.
