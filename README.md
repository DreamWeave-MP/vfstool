# vfstool

`vfstool` is a command-line utility for interacting with OpenMW's virtual file system (VFS). It allows users to locate files, serialize the VFS to various formats, extract files, and even collapse the VFS into a single directory for space savings, and to reuse OpenMW's feature set for other games.

## Features

- **Collapse the VFS**: Create a set of hardlinks or symbolic links for the entire VFS in a target directory.
- **Extract Files**: Extract specific files from the VFS to a given directory.
- **Find Files**: Locate files in the VFS by name, extension, or other criteria.
- **Serialize the VFS**: Output the VFS structure in JSON, YAML, or TOML formats.
- **Filter Remaining Files**: Identify files in a directory that are replaced or not replaced by the VFS.
- **Conflict and provider reports**: Inspect winners, fully shadowed sources, duplicates, archives, per-source contributions, and source-to-source diffs.
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

## 1.0 breaking API changes for Rust consumers

The `vfstool` CLI keeps the same user-facing archive behavior, but the library API was cleaned up
for 1.0. If you depend on `vfstool_lib` directly, check these before upgrading:

- VFS keys are byte-first normalized resource paths now. Use `vfstool_lib::NormalizedPath` and
  `VfsKeyInput` for VFS keys; keep using `Path`/`PathBuf` for real host filesystem paths. These are
  not the same thing, despite many older APIs pretending they were. They were lying.
- Bethesda archive support is provided by `dream_archive`; enable it with the `beth-archives`
  feature. The old `bsa` feature name is gone because the feature covers both BSA and BA2.
- `vfstool_lib` re-exports `serde`, `serde_json`, `serde_yaml`, and `toml` when `serialize` is
  enabled, so applications can share the library's serialization stack instead of pinning duplicate
  parser versions.
- ZIP/PK3 support uses `zip` without default features. Currently supported ZIP compression is stored,
  deflate, and LZMA; AES, bzip2, PPMd, deflate64, and zstd are intentionally not dragged into the
  dependency graph.

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
- `--dry-run`: Print the planned materialization actions instead of writing files.
- `-f, --format <FORMAT>`: Output format for `--dry-run` (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the `--dry-run` plan. If omitted, results are printed to stdout.

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

Report sources whose files are all overridden by higher-priority sources.

```bash
vfstool shadowed [OPTIONS]
```

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the report. If omitted, results are printed to stdout.

---

#### Provider reports

Inspect the provider index used by the resolved VFS. These commands are projections over the same VFS provider data; they are not a second conflict system wearing a fake moustache.

```bash
vfstool explain [OPTIONS] <PATH>
vfstool duplicates [OPTIONS] [PATTERN]
vfstool archives [OPTIONS]
vfstool archive-list [OPTIONS] <ARCHIVE>
vfstool case-collisions [OPTIONS]
vfstool contributions [OPTIONS]
vfstool validate [OPTIONS]
```

**Commands**:

- `explain <PATH>`: Show the winning provider and lower-priority providers for one VFS key.
- `duplicates [PATTERN]`: List VFS keys with more than one provider. `PATTERN`, when supplied,
  is a case-insensitive regex over normalized VFS keys, e.g. `^meshes/` or `textures/.*\\.dds$`.
- `archives`: List loaded archives and how many entries currently win.
- `archive-list <ARCHIVE>`: List VFS entries supplied by one archive.
- `case-collisions`: Report distinct original path spellings that normalize to the same VFS key.
- `contributions`: Report per-source provider counts, wins, overridden files, unique files, and duplicates.
- `validate`: Report missing loose winners, file/directory materialization conflicts, and case collisions.

**Options**:

- `-f, --format <FORMAT>`: Output format (`json`, `yaml`, or `toml`). Default: `yaml`.
- `-o, --output <OUTPUT>`: Path to save the report. If omitted, results are printed to stdout.

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

## Exit codes

`vfstool` uses nonzero exit codes for machine-readable failure cases:

| Code | Meaning |
|------|---------|
| `1` | `find-file` did not find the requested VFS path |
| `2` | `find-file --only_physical` found the path only inside an archive |
| `4` | `drift --fail-on-drift` detected drift |
| `6` | invalid regular expression |
| `7` | failed to load `openmw.cfg` |
| `8` | invalid input, such as an unknown source path |
| `9` | runtime failure while reading, writing, materializing, or running a child command |

---

## Examples

The published rustdoc for the `vfstool` binary also contains operational examples for configuration,
provider inspection, lock/drift, collapse previews, and `run`. The README is the quick path; rustdoc
is the self-documenting reference we try not to let rot.

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

### Explain why a file resolves to its winner

```bash
vfstool explain textures/tx_bc_mudcrab.dds
```

### See which sources contribute winners or get overridden

```bash
vfstool contributions -f json -o contributions.json
```

### Validate the provider index before materializing

```bash
vfstool validate
```

### Preview a collapse without writing files

```bash
vfstool collapse --dry-run -f yaml /tmp/merged-preview
```

### Lock current winners and fail later if they drift

```bash
vfstool lock -o vfs-lock.yaml
vfstool drift --fail-on-drift vfs-lock.yaml
```

---

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

---
