+++
title = "Materializing and trees"
description = "Writing a VFS out as one directory: collapse_into, dump_to_directory, extract_file and the dry-run plan; and the sorted trees, text listings and JSON, YAML or TOML output the CLI prints."
weight = 40

[extra]
kind = "api"
+++

A [`VFS`](@/docs/api/vfs.md) can be written out: the whole of it as one directory, or one file.
It can also be shown, as a sorted tree of directories and files, printed as text or serialized.
`vfstool collapse`, `extract`, `find` and `remaining` are these calls.

```rust
use std::fs;
use std::path::Path;

use vfstool_lib::{CollapseOptions, MaterializationAction, VFS};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-collapse");
    let base = root.join("base");
    let patch = root.join("patch");
    fs::create_dir_all(base.join("Meshes"))?;
    fs::create_dir_all(patch.join("meshes"))?;
    fs::write(base.join("Meshes/Door.NIF"), "base")?;
    fs::write(base.join("Morrowind.esm"), "esm")?;
    fs::write(patch.join("meshes/door.nif"), "patch")?;
    let vfs = VFS::from_directories([&base, &patch], None);

    let options = CollapseOptions {
        allow_copying: true,
        extract_archives: false,
        use_symlinks: false,
    };
    let merged = root.join("merged");

    // What would happen, without writing anything.
    let plan = vfs.materialization_plan(&merged, &options);
    assert!(plan.issues.is_empty());
    assert!(matches!(plan.actions[0], MaterializationAction::Hardlink { .. }));

    vfs.collapse_into(&merged, &options)?;
    assert_eq!(fs::read_to_string(merged.join("meshes/door.nif"))?, "patch");
    assert!(merged.join("morrowind.esm").is_file());

    let extracted = vfs.extract_file(Path::new("Meshes/Door.NIF"), &root.join("out"))?;
    assert_eq!(extracted, Some(root.join("out/door.nif")));

    fs::remove_dir_all(&root)
}
```

## Writing files out

Every winner is written at its key under the destination, so file and directory names come out
lowercase: `Meshes/Door.NIF` becomes `meshes/door.nif`. Losing providers are never written.

All three methods refuse to write through a symbolic link: if the destination is a symbolic link,
or a directory already under it on the way to a file is one, the call fails with `InvalidInput`.
A file already at a destination is replaced; a directory there is an error. A loose file whose
destination already is that file is left alone: the same path, another spelling of it on a file
system that ignores case, a hard link to it, or a symbolic link to it. Writing into one of the
VFS's own data directories therefore keeps that directory's files and adds the rest around them,
and extracting a file into the folder it lives in leaves it there. Files are written in parallel, so when a call fails, some
others may already be written.

{{ api_signature(value="fn collapse_into(&self, dest: &Path, opts: &CollapseOptions) -> io::Result<()>") }}

Writes the whole VFS into `dest`, which is created if needed, the way `vfstool collapse` does.

- **Loose files** are hard-linked, or symbolically linked with `use_symlinks`. When linking fails,
  for example across file systems, the file is copied with `allow_copying` and the call fails
  without it.
- **Archive entries** are extracted with `extract_archives`, read through
  [`VfsFile::open`](@/docs/api/files.md#reading-it). Without it they are left out, and each is reported
  on standard error: `vfstool: skipping KEY, loaded from archive: ARCHIVE`.
- **Archives that are loose files**, a `.bsa` or `.ba2` with `beth-archives` or a `.zip` or `.pk3`
  with `zip`, are left out with `extract_archives`, so the result does not hold both an archive and
  its contents. `ArchiveInvalidationInvalidated!.bsa` is not treated as an archive. Without
  `extract_archives`, they are linked like any file.

A loose file that no longer exists is an error. A symbolic link points at the file's path as the
VFS stores it, so a VFS built from relative directories makes links that resolve relative to
where they are, and break; build it from absolute paths when linking.

Hard links share their bytes with the original: a program that edits a collapsed file in place
edits the file in the mod.

{{ api_signature(value="struct CollapseOptions") }}

Three public fields and no derives, not even `Default`: set all three.

| Field | Meaning | CLI |
|---|---|---|
| `allow_copying: bool` | Copy a loose file when linking it fails | `--allow-copying` |
| `extract_archives: bool` | Extract archive entries, and leave out loose archive files | `--extract-archives` |
| `use_symlinks: bool` | Symbolic links instead of hard links | `--symbolic` |

{{ api_signature(value="fn dump_to_directory(&self, dir: &Path, use_hardlinks: bool) -> std::io::Result<usize>") }}

Writes every winner into `dir` and returns how many files were written.
[`run_setup`](@/docs/api/run.md) writes the same way, except that a missing file is an error there.
It differs from `collapse_into` in three ways:

- **Archive entries are always extracted.** There is no option to leave them out.
- **Loose files are hard-linked with `use_hardlinks`, copied without it**, and a hard link that
  fails because it would cross file systems falls back to a copy. Other link errors fail the call.
- **Missing files are skipped, not errors.** A loose file that no longer exists, or an archive
  entry that does not open, is left out and reported on standard error, as
  `vfstool: skipping KEY: source no longer exists at PATH` or `vfstool: skipping KEY: ERROR`, and
  not counted.

`dir` is created if it does not exist.

{{ api_signature(value="fn extract_file(&self, vfs_path: &Path, dest_dir: &Path) -> io::Result<Option<PathBuf>>") }}

Writes the file that wins `vfs_path` into `dest_dir`, created if needed, under the key's own file
name, lowercase: `Meshes/XBase_Anim.NIF` is written as `xbase_anim.nif`. A loose file is copied,
an archive entry extracted. Returns the path written, or `Ok(None)` when the key does not resolve.
A loose file that has gone missing since the VFS was built is an error.

## Planning without writing

{{ api_signature(value="fn materialization_plan(&self, dest: impl AsRef<Path>, opts: &CollapseOptions) -> MaterializationPlan") }}

What `collapse_into(dest, opts)` would do, in key order, without touching the disk beyond checking
that loose files exist and that `dest` holds no symbolic links. `vfstool collapse --dry-run`
prints it.

For each winner, in order of checks:

1. A destination that cannot be written, or that goes through a symbolic link, is an
   `UnsafeDestination` issue.
2. A loose file that does not exist is a `MissingLooseSource` issue.
3. A loose file that is an archive, with `extract_archives`, is `SkipArchiveFile`.
4. Any other loose file is `Symlink` with `use_symlinks`, and `Hardlink` otherwise.
   `allow_copying` changes nothing here: the plan shows the link, which is tried first.
5. An archive entry is `ExtractArchive` with `extract_archives`, and `SkipArchiveFile` otherwise.

The plan never contains `Copy` or `FileDirectoryConflict`; both exist so that later planners can
report them without changing the types.

{{ api_signature(value="struct MaterializationPlan") }}

`Debug`, `Clone`; `Serialize` with `serialize`.

| Field | Type | Meaning |
|---|---|---|
| `actions` | `Vec<MaterializationAction>` | One per file to write, sorted by key |
| `issues` | `Vec<MaterializationIssue>` | One per file that could not be planned |

{{ api_signature(value="enum MaterializationAction") }}

`Debug`, `Clone`; `Serialize` with `serialize`, with each variant as a `snake_case` tag:
`{"hardlink": {"key": …, "source": …, "dest": …}}`.

| Variant | Means |
|---|---|
| `Hardlink { key: PathBuf, source: PathBuf, dest: PathBuf }` | Hard-link the loose file `source` at `dest` |
| `Symlink { key: PathBuf, source: PathBuf, dest: PathBuf }` | Symbolically link it |
| `Copy { key: PathBuf, source: PathBuf, dest: PathBuf }` | Copy it. Not produced yet |
| `ExtractArchive { key: PathBuf, archive: PathBuf, dest: PathBuf }` | Extract the entry for `key` from `archive` to `dest` |
| `SkipArchiveFile { key: PathBuf, archive: PathBuf }` | Leave it out. `archive` is the archive the entry is in, or the loose archive file itself |

{{ api_signature(value="enum MaterializationIssue") }}

`Debug`, `Clone`; `Serialize` with `serialize`, tagged like `MaterializationAction`.

| Variant | Means |
|---|---|
| `MissingLooseSource { key: PathBuf, source: PathBuf }` | The loose file `source` does not exist |
| `FileDirectoryConflict { key: PathBuf, dest: PathBuf }` | `dest` would be a file and a directory. Not produced yet |
| `UnsafeDestination { key: PathBuf, dest: PathBuf }` | `dest` goes through a symbolic link, or the key cannot be a path on this platform |

## Trees

{{ api_signature(value="type DisplayTree = BTreeMap<PathBuf, DirectoryNode>") }}

A sorted directory tree, at the crate root: each entry is one directory name and its node. The
trees on this page have one root entry. With `relative`, it is `Data Files`, and files are laid out
by key, with archive entries under their archive's file name: `Data Files/meshes/x/door.nif`,
`Data Files/Morrowind.bsa/meshes/x/door.nif`. Without it, the root is `/`, and files are laid out by
their real paths, and archive entries under their archive's path. The root entry is there even when
no file is.

{{ api_signature(value="struct DirectoryNode") }}

In the `directory_node` module. `Debug`, `Default`; with `serialize`, `Serialize` as a map: a
`"."` entry listing the node's file names, when it has files, then one entry per subdirectory.

| Field | Type | Meaning |
|---|---|---|
| `files` | `Vec<VfsFile>` | The files directly in this directory |
| `subdirs` | `DisplayTree` | Its subdirectories, by name |

Files are shown by their own [`file_name`](@/docs/api/files.md#its-path): the loose file's name
in its own case, or the archive entry's. So a relative tree has the directory `meshes/x`, from the
key, holding `Door.NIF`, from the disk.

{{ api_signature(value="fn new() -> DirectoryNode") }}

An empty node.

{{ api_signature(value="fn sort(&mut self)") }}

Sorts `files` by file name, byte order, and every subdirectory the same way. Trees from the
methods below come sorted.

{{ api_signature(value="fn filter<F>(&mut self, file_filter: &F) where F: Fn(&VfsFile) -> bool") }}

Keeps the files `file_filter` accepts, recursively, and drops subdirectories left with no files
and no subdirectories.

{{ api_signature(value="fn tree(&self, relative: bool) -> DisplayTree") }}

Every winner as a sorted tree.

{{ api_signature(value="fn tree_filtered(&self, relative: bool, file_filter: impl Fn(&NormalizedPath, &VfsFile) -> bool) -> DisplayTree") }}

The winners `file_filter` accepts, given each key and file, as a sorted tree. Directories are only
created for files that pass, so there is nothing empty to prune. `find_by_regex` and `remaining`
are built on it.

{{ api_signature(value="fn display_filtered(&self, relative: bool, file_filter: impl Fn(&NormalizedPath, &VfsFile) -> bool) -> String") }}

`tree_filtered` as text: every directory that holds files, by its own name rather than its path,
followed by its files, depth first. A directory with only subdirectories is not printed. `VFS`'s
`Display` is this with every file and `relative`:

```text
├── Data Files/
│   ├── Morrowind.esm
├── x/
│   ├── Door.NIF
```

A file whose path has no file name, such as a `VfsFile::from("/")` set by hand, is listed by its
whole path, here and in serialized trees.

{{ api_signature(value="fn serialize_from_tree(tree: &DisplayTree, write_type: SerializeType) -> Result<String>") }}

With `serialize`. An associated function, not a method: `VFS::serialize_from_tree(&tree, …)`.
Serializes a tree as compact JSON, YAML or pretty TOML. The `Result` is `std::io::Result`: a
serializer error comes back as `InvalidData`. For the tree above:

```json
{"Data Files":{".":["Morrowind.esm"],"meshes":{"x":{".":["Door.NIF"]}}}}
```

```rust
use std::fs;

use vfstool_lib::{SerializeType, VFS};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-tree");
    fs::create_dir_all(root.join("Meshes/X"))?;
    fs::write(root.join("Meshes/X/Door.NIF"), "nif")?;
    fs::write(root.join("Morrowind.esm"), "esm")?;
    let vfs = VFS::from_directories([&root], None);

    let json = VFS::serialize_from_tree(&vfs.tree(true), SerializeType::Json)?;
    assert_eq!(json, r#"{"Data Files":{".":["Morrowind.esm"],"meshes":{"x":{".":["Door.NIF"]}}}}"#);

    let only_meshes = vfs.display_filtered(true, |key, _| key.as_bytes().starts_with(b"meshes/"));
    assert_eq!(only_meshes, "├── x/\n│   ├── Door.NIF\n");

    fs::remove_dir_all(&root)
}
```

## Serializing anything

{{ api_signature(value="enum SerializeType") }}

`Json`, `Yaml` or `Toml`. `Debug`, `Clone`, `Copy`. At the crate root, and present without the
`serialize` feature, which the functions that take it need.

{{ api_signature(value="fn serialize_value<T: serde::Serialize>(value: &T, write_type: SerializeType) -> std::io::Result<String>") }}

With `serialize`, at the crate root. Serializes any value, such as a report from this API, as
compact JSON, YAML or pretty TOML, the way the CLI writes every report. The serializer's error comes
back as `InvalidData`. TOML needs a table at the top: a bare list, such as the `Vec<ArchiveInfo>`
from `archives()`, fails with `unsupported array type`, while a struct holding lists works.

```rust
use std::fs;

use vfstool_lib::{SerializeType, VFS, serialize_value};

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-serialize");
    fs::create_dir_all(&root)?;
    fs::write(root.join("Morrowind.esm"), "esm")?;
    let vfs = VFS::from_directories([&root], None);

    let explain = vfs.explain("morrowind.esm").unwrap();
    let yaml = serialize_value(&explain, SerializeType::Yaml)?;
    assert!(yaml.starts_with("key: morrowind.esm\n"));

    assert!(serialize_value(&vfs.archives(), SerializeType::Toml).is_err());

    fs::remove_dir_all(&root)
}
```
