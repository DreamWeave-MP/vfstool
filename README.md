# vfstool

`vfstool` is a command-line utility for interacting with OpenMW's virtual file system (VFS). It allows users to locate files, serialize the VFS to various formats, extract files, and even collapse the VFS into a single directory for space savings, and to reuse OpenMW's feature set for other games.

## Features

- **Collapse the VFS**: Create a set of hardlinks or symbolic links for the entire VFS in a target directory.
- **Extract Files**: Extract specific files from the VFS to a given directory.
- **Find Files**: Locate files in the VFS by name, extension, or other criteria.
- **Serialize the VFS**: Output the VFS structure in JSON, YAML, or TOML formats.
- **Filter Remaining Files**: Identify files in a directory that are replaced or not replaced by the VFS.

---

## Installation

As of version 0.1.6, vfstool is published in many places!

### GitHub

The latest stable release can be downloaded from GitHub for macOS, Linux, and Windows [here](https://github.com/magicaldave/vfstool/releases/latest). Development builds can be found at [here](https://github.com/magicaldave/vfstool/releases/development).

### AUR

`yay -S vfstool-git`.

### crates.io

`cargo install vfstool`

### source

Clone the repository and build the tool using `cargo`:

```bash
git clone https://github.com/magicaldave/vfstool.git
cd vfstool
cargo install --path .
```

---

## Usage

```bash
vfstool [OPTIONS] <COMMAND>
```

### Global Options

- `-c, --config <CONFIG>`: Path to the directory containing `openmw.cfg`. If omitted, the system default location is used.
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

- `-s, --simple`: Output the result in a simple format (no colors or formatting).

---

#### `find`

Search for files in the VFS based on a query term.

```bash
vfstool find [OPTIONS] --path <PATH>
```

**Options**:

- `-p, --path <PATH>`: Query term, actual contents depend on search type. Mandatory
- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the search results. If omitted, results are printed to stdout.
- `-t, --type <TYPE>`: Type of filter to use when searching. Default: `name`.

**Filter Types**:

- `exact`: Match the exact VFS path.
- `name`: Match files containing the query in their name.
- `name-exact`: Match files with the exact name.
- `folder`: Match files in a specific folder.
- `prefix`: Match files with a specific prefix.
- `extension`: Match files with a specific extension.
- `stem`: Match files by their stem (filename without extension).
- `stem-exact`: Match files by their exact stem.

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

## Examples

### Collapse the VFS into a directory with symlinks

```bash
vfstool collapse -s /path/to/target
```

This form is the most space-efficient variant of collapse, since it doesn't copy or extract files. It's fragile and most ideal for testing mods.

### Collapse the VFS into a single directory, with extraction and hardlinks

```bash
vfstool -c C:\Games\Oblivion\openmw.cfg collapse -ae C:\Games\Oblivion\Data
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
vfstool find -t extension -f json -o results.json nif
```

### Show files replacing contents of a directory

```bash
vfstool remaining -r /path/to/filter
```

---

## License

This project is licensed under the MIT License. See the LICENSE file for details.

---
