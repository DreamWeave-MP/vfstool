+++
title = "VfsFile"
description = "VfsFile, the file behind a VFS key: a loose path or an archive entry, its constructors, what its path and name methods return, and how open reads it."
weight = 50

[extra]
kind = "api"
+++

A `VfsFile` is where a key's bytes are: a loose file on disk, or an entry in an open archive. It is
what [`get_file`](@/docs/api/vfs.md#looking-files-up) returns and what a
[`VfsProvider`](@/docs/api/mutation.md#vfsprovider) holds. It does not know its key, and its path
is never normalized: a loose file keeps the host path it was found at, and an archive entry the
name the archive stores.

```rust
use std::fs;
use std::io::Read;

use vfstool_lib::VFS;

fn main() -> std::io::Result<()> {
    let root = std::env::temp_dir().join("vfstool-doc-vfsfile");
    fs::create_dir_all(root.join("Meshes"))?;
    fs::write(root.join("Meshes/Ex_Door.NIF"), "door")?;
    let vfs = VFS::from_directories([&root], None);

    let file = vfs.get_file("meshes/ex_door.nif").unwrap();
    assert!(file.is_loose());
    assert_eq!(file.path(), root.join("Meshes/Ex_Door.NIF"));
    assert_eq!(file.file_name().unwrap(), "Ex_Door.NIF");

    let mut bytes = Vec::new();
    file.open()?.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"door");

    fs::remove_dir_all(&root)
}
```

## VfsFile

{{ api_signature(value="struct VfsFile") }}

`Debug`, `Clone`, `Default` (a loose file with an empty path). Cloning an archive entry shares
the open archive. Not `Serialize`: reports carry paths instead.

## Constructing

A VFS makes its own files. These are for building providers by hand, for
[`set_winner_file`](@/docs/api/mutation.md#adding) and
[`push_provider`](@/docs/api/mutation.md#adding).

{{ api_signature(value="fn from<P: AsRef<Path>>(path: P) -> VfsFile") }}

A loose file at `path`, kept exactly as given. Nothing is checked: a file that does not exist
fails when it is opened. This is an associated function named `from`, not the `From` trait, so
`"x".into()` does not make a `VfsFile`.

{{ api_signature(value="fn from_archive<S: AsRef<str>>(path: S, parent_archive: Arc<StoredArchive>) -> VfsFile") }}

With `beth-archives` or `zip`. An entry named `path` in a Bethesda archive. For a ZIP or PK3, use
`from_zip_archive`: a file made by this function in a ZIP cannot be opened, and `open` returns
`InvalidData` (`zip archive reference is missing central-directory index`).

{{ api_signature(value="fn from_zip_archive<S: AsRef<str>>(path: S, zip_index: usize, parent_archive: Arc<StoredArchive>) -> VfsFile") }}

With `zip`. The entry at `zip_index` in the archive's central directory. The index is what `open`
reads, because a ZIP can hold two entries with the same name; `path` is the name the file shows.

{{ api_signature(value="fn from_archive_bytes(path: &[u8], parent_archive: Arc<StoredArchive>) -> VfsFile") }}

With `beth-archives` or `zip`. `from_archive` for an entry name that is not UTF-8, as old BSAs
have. The bytes are kept exactly; `path()` shows them with invalid bytes replaced, and
`path_bytes()` returns them as they are. How the VFS itself makes Bethesda entries.

The archive handle these take comes from the [`archives`](@/docs/api/archives.md) module.

## What it is

{{ api_signature(value="fn is_loose(&self) -> bool") }}

Whether it is a file on disk.

{{ api_signature(value="fn is_archive(&self) -> bool") }}

Whether it is an archive entry. Always `false` without an archive feature.

{{ api_signature(value="fn parent_archive_path(&self) -> Option<String>") }}

The archive's path, as it was opened, as text (bytes that are not UTF-8 replaced). `None` for a
loose file.

{{ api_signature(value="fn parent_archive_name(&self) -> Option<String>") }}

The archive's file name, such as `Morrowind.bsa`. `None` for a loose file.

{{ api_signature(value="fn parent_archive_handle(&self) -> io::Result<Arc<StoredArchive>>") }}

With `beth-archives` or `zip`. The open archive, shared. For a loose file, `InvalidData`
(`Loose files may not return an archive reference!`).

## Its path

{{ api_signature(value="fn path(&self) -> &Path") }}

For a loose file, its host path as given. For an archive entry, the entry name as a `Path`, such as
`meshes\x\ex_door.nif` from a Morrowind BSA, with bytes that are not UTF-8 replaced.

{{ api_signature(value="fn path_bytes(&self) -> &[u8]") }}

The same path as exact bytes: the host path's encoded bytes for a loose file, the stored name for
an archive entry. Nothing replaced.

{{ api_signature(value="fn file_name(&self) -> Option<&std::ffi::OsStr>") }}

The last component of `path()`. For a loose file, its name in its own case: `Ex_Door.NIF`. For
an archive entry, the part after the last `\` or `/` of its entry name, on every platform: a
Morrowind BSA's `meshes\x\ex_door.nif` is `ex_door.nif`. `None` when there is none, as for `/`
or an entry name that ends in a separator.

{{ api_signature(value="fn file_stem(&self) -> Option<&std::ffi::OsStr>") }}

`file_name` without its extension.

## Reading it

{{ api_signature(value="fn open(&self) -> io::Result<Box<dyn Read + '_>>") }}

A reader over the file's bytes.

- **Loose files** stream from the file. A file that is gone is the system's error.
- **BSA and BA2 entries** stream through dream_archive. An entry that cannot be read is
  `InvalidData`.
- **ZIP and PK3 entries** are read whole into memory when opened, then handed out. An entry larger
  than 512 MiB uncompressed is refused with `OutOfMemory`. Reads from one ZIP take turns: its
  handle is behind a lock. Supported compression is stored, deflate and LZMA.

## FileType

{{ api_signature(value="enum FileType") }}

In the `vfs_file` module: what a `VfsFile` holds. `Debug`, `Clone`. `VfsFile` does not hand it
out; it is public so the type can be named.

| Variant | Present | Holds |
|---|---|---|
| `Archive(ArchiveReference)` | with `beth-archives` or `zip` | An entry in an open archive |
| `Loose(PathBuf)` | always | A host path, as given |

## ArchiveReference

{{ api_signature(value="struct ArchiveReference") }}

In the `vfs_file` module, with `beth-archives` or `zip`. One entry in a
[`StoredArchive`](@/docs/api/archives.md#storedarchive): its name, its exact bytes, its ZIP index
if it has one, and the shared archive. `Debug`, `Clone`. Its fields are private and it has no
public methods; `VfsFile` is the way in.
