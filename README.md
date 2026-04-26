# vfstool

`vfstool` is a command-line utility for interacting with OpenMW's virtual file system (VFS). It allows users to locate files, serialize the VFS to various formats, extract files, and even collapse the VFS into a single directory for space savings, and to reuse OpenMW's feature set for other games.

## Features

- **Collapse the VFS**: Create a set of hardlinks or symbolic links for the entire VFS in a target directory.
- **Extract Files**: Extract specific files from the VFS to a given directory.
- **Find Files**: Locate files in the VFS by name, extension, or other criteria.
- **Serialize the VFS**: Output the VFS structure in JSON, YAML, or TOML formats.
- **Filter Remaining Files**: Identify files in a directory that are replaced or not replaced by the VFS.
- **Conflict reports**: Inspect winners, shadowed files, per-source stats, and source-to-source diffs.
- **Lock/drift checks**: Emit a deterministic winner manifest and compare later VFS state against it.
- **Run tools against a merged VFS**: Dump a merged tree, execute a child command, then capture new or modified output files.

---

## Installation

As of version 1.0, vfstool is published in many places.

### GitHub

The latest stable release can be downloaded from GitHub for macOS, Linux, and Windows [here](https://github.com/DreamWeave-MP/vfstool/releases/latest). Development builds can be found [here](https://github.com/DreamWeave-MP/vfstool/releases/development).

### AUR

`yay -S vfstool-git`.

### crates.io

`cargo install vfstool`

### source

Clone the repository and build the tool using `cargo`:

```bash
git clone https://github.com/DreamWeave-MP/vfstool.git
cd vfstool
cargo install --path vfstool
```

---

## Usage

```bash
vfstool [OPTIONS] <COMMAND>
```

### Global Options

- `-c, --config <CONFIG>`: Path to the directory containing `openmw.cfg`. If omitted, the system default location is used.
  For a config file with a nonstandard name, set `OPENMW_CONFIG` to the absolute file path instead.
- `-r, --use-relative`: Use relative paths in output.
- `-h, --help`: Describe usage of the app or any subcommand

---

### Commands

#### `collapse`

Collapse the VFS into a target directory using hardlinks, symbolic links, or file copies.

```bash
vfstool collapse [OPTIONS] <COLLAPSE_INTO>
```

**Options**:

- `<COLLAPSE_INTO>`: Target folder to collapse the VFS into.
- `-a, --allow-copying`: Fall back to copying files if linking fails.
- `-e, --extract-archives`: Extract files from BSA/BA2 archives during collapsing.
- `-s, --symbolic`: Use symbolic links instead of hardlinks.

---

#### `extract`

Extract a specific file from the VFS into a target directory.

```bash
vfstool extract <SOURCE_FILE> <TARGET_DIR>
```

**Arguments**:

- `<SOURCE_FILE>`: Full relative path to a VFS file (e.g., `meshes/xbase_anim.nif`).
- `<TARGET_DIR>`: Directory to extract the file to.

---

#### `find-file`

Locate a specific file in the VFS and return its absolute or relative path.

```bash
vfstool find-file [OPTIONS] <PATH>
```

**Arguments**:

- `<PATH>`: Full (relative) VFS path to query.

**Options**:

- `-p, --only_physical`: Match only loose files; exits `2` if the file exists only in an archive.
- `-s, --simple`: Output the result in a simple format (no colors or formatting).

---

#### `find`

Search for files in the VFS using a case-insensitive regular expression matched against normalized VFS paths.

```bash
vfstool find [OPTIONS] <PATH>
```

**Arguments**:

- `<PATH>`: Case-insensitive regex matched against VFS paths.

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the search results. If omitted, results are printed to stdout.

---

#### `remaining`

Filter the VFS to show files replacing or not replacing contents of a given directory.

```bash
vfstool remaining [OPTIONS] <FILTER_PATH>
```

**Arguments**:

- `<FILTER_PATH>`: Absolute path to filter against.

**Options**:

- `-r, --replacements-only`: Show only files replacing contents of the given path.
- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the filtered VFS. If omitted, results are printed to stdout.

---

#### `conflicts`

Report source override relationships across the load order.

```bash
vfstool conflicts [OPTIONS]
```

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the report. If omitted, results are printed to stdout.

---

#### `shadowed`

Report files overridden by higher-priority sources.

```bash
vfstool shadowed [OPTIONS]
```

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the report. If omitted, results are printed to stdout.

---

#### `which`

Show the winning source for one VFS path and any lower-priority providers.

```bash
vfstool which <PATH>
```

---

#### `stats`

Show per-source winner, override, and overridden counts.

```bash
vfstool stats
```

---

#### `diff`

Compare files between two configured data directories.

```bash
vfstool diff [OPTIONS] <SOURCE_A> <SOURCE_B>
```

**Arguments**:

- `<SOURCE_A>`: First configured data directory to compare.
- `<SOURCE_B>`: Second configured data directory to compare.

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the report. If omitted, results are printed to stdout.

---

#### `run`

Dump the merged VFS to a directory, run a child command, then capture new or modified files to `data-local` or `--output`.

```bash
vfstool run [OPTIONS] <MERGED_DIR> -- <COMMAND>...
```

**Options**:

- `--keep-merged`: Keep the merged directory after the child command exits.
- `--output <OUTPUT>`: Destination for captured files. Defaults to `data-local` from `openmw.cfg`.
- `--copy`: Copy files instead of hardlinking them into the merged directory.
- `--working-dir <WORKING_DIR>`: Working directory for the child process.

`{}` in child command arguments is replaced with the merged directory path. Deletions made by the child command are not captured.

By default, `run` uses hardlinks when dumping loose files into the merged directory. This avoids duplicating data, but child tools that modify files in place may modify the original loose source files through those hardlinks. Use `--copy` for tools that are not hardlink-safe. No, that is not a theoretical footgun. It is just how hardlinks work.

---

#### `lock`

Emit a deterministic lock manifest for current VFS winners.

```bash
vfstool lock [OPTIONS]
```

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the lock file. If omitted, results are printed to stdout.

---

#### `drift`

Compare the current VFS state to a lock manifest.

```bash
vfstool drift [OPTIONS] <LOCK_FILE>
```

**Options**:

- `--fail-on-drift`: Exit with code `4` when drift is detected.
- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the report. If omitted, results are printed to stdout.

---

## Examples

### Collapse the VFS into a directory with symlinks

```bash
vfstool collapse -s /path/to/target
```

This form is the most space-efficient variant of collapse, since it doesn't copy or extract files. It's fragile and most ideal for testing mods.

### Collapse the VFS into a single directory, with extraction and hardlinks

```bash
vfstool -c C:\Games\Oblivion collapse -ae C:\Games\Oblivion\Data
```

This form consumes more space and takes longer due to extracting archive contents, but will perform better ingame and allow removing BSAs entirely.

### Extract a file from the VFS

```bash
vfstool extract meshes/xbase_anim.nif /path/to/output
```

### Find a file in the VFS

```bash
vfstool find-file meshes/xbase_anim.nif
```

### Search for files by extension

```bash
vfstool find -f json -o results.json '[.]nif$'
```

### Show files replacing contents of a directory

```bash
vfstool remaining -r /path/to/filter
```

### Run a tool against a merged VFS

```bash
vfstool run /tmp/merged -- some-tool {} output.txt
```

---

## License

This project is dual-licensed under either:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

You may choose either license at your option.

---
