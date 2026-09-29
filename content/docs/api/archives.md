+++
title = "Archives"
description = "Which archives vfstool_lib opens and with which features, how their entries become keys, and the low-level archives module: from_set, file_entries, file_map, archive_paths, StoredArchive and TypedArchive."
weight = 130

[extra]
kind = "api"
+++

With the `beth-archives` or `zip` feature, a [`VFS`](@/docs/api/vfs.md) reads archives:
[`from_directories`](@/docs/api/vfs.md#building-one) opens the ones named in its archive list
and puts their entries below every loose file, and
[`push_archive`](@/docs/api/vfs.md#building-one) stacks one on top. Most programs need nothing
more. The `archives` module underneath is public for programs that open archives themselves.

```rust
use std::fs;
use std::io::Write;

use vfstool_lib::{VFS, archives};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join("vfstool-doc-archives");
    let data = root.join("Data Files");
    fs::create_dir_all(data.join("meshes"))?;
    fs::write(data.join("meshes/door.nif"), "loose")?;

    // A PK3 holding two files, one of which a loose file also provides.
    let mut pk3 = ZipWriter::new(fs::File::create(data.join("Doors.pk3"))?);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in [("Meshes/Door.NIF", "packed"), ("Meshes/Gate.NIF", "packed")] {
        pk3.start_file(name, stored)?;
        pk3.write_all(bytes.as_bytes())?;
    }
    pk3.finish()?;

    let vfs = VFS::from_directories([&data], Some(vec!["doors.pk3"]));

    // Loose files win over archive entries.
    assert!(vfs.get_file("meshes/door.nif").unwrap().is_loose());
    let gate = vfs.get_file("meshes/gate.nif").unwrap();
    assert!(gate.is_archive());
    assert_eq!(gate.parent_archive_name().as_deref(), Some("Doors.pk3"));

    // Every entry the archive supplies, winning or not.
    assert_eq!(vfs.files_from_archive(data.join("Doors.pk3")).len(), 2);

    // The archive itself, and its keys.
    let archive = gate.parent_archive_handle()?;
    let mut keys = archives::archive_paths(&archive);
    keys.sort();
    assert_eq!(keys, [std::path::Path::new("meshes/door.nif"), std::path::Path::new("meshes/gate.nif")]);

    fs::remove_dir_all(&root)?;
    Ok(())
}
```

The example writes its PK3 with the `zip` crate, so it needs
`zip = { version = "8", default-features = false }` beside
`vfstool_lib = { version = "1", features = ["zip"] }`.

## What opens

| Feature | Opens | Through |
|---|---|---|
| `beth-archives` | Morrowind's BSA; the BSA of Oblivion, Fallout 3, New Vegas and Skyrim; BA2 general and texture archives from Fallout 4 on | [dream_archive](https://crates.io/crates/dream_archive) 0.2 |
| `zip` | `.zip` and `.pk3` files, stored, deflate or LZMA | [zip](https://crates.io/crates/zip) 8, without its default features |

A file whose name ends in `.zip` or `.pk3`, in any case, is opened as a ZIP with the `zip`
feature. Any other file is opened as a Bethesda archive with `beth-archives`, recognized by its
contents rather than its extension. A file that does not open is left out without an error by
`from_directories` and `from_set`, and makes `push_archive` return `false`. With only `zip`, a BSA
does not open; with only `beth-archives`, a PK3 does not.

ZIP entries compressed with AES, bzip2, deflate64, PPMd or zstd do not read. ZIP entries are
buffered whole when opened, up to 512 MiB each, uncompressed; see
[`VfsFile::open`](@/docs/api/files.md#reading-it). BSA and BA2 entries stream.

## How entries become keys

Each entry's stored name is normalized into a key, like a loose file's relative path. A name that
is not a safe key (absolute, `..`, a drive letter, NUL) is skipped, and so are ZIP directory
entries. An Oblivion-family BSA entry whose full name is not stored is named from its folder and
file name, joined with `\`. Two entries that normalize to one key are both kept, as two providers,
and the later one wins.

Among archives, the one later in the list wins. Every archive is below every loose file when
built by `from_directories`, whatever the list's order.

## The archives module

Present with `beth-archives` or `zip`. Nothing here consults loose-file priority; it opens archives
and lists their entries.

{{ api_signature(value="fn from_set(file_map: &AHashMap<NormalizedPath, VfsFile>, archive_list: &[&str]) -> ArchiveList") }}

Opens every archive in `archive_list` that `file_map` can resolve, in the list's order. Each name
is normalized and looked up as a key in `file_map`, and the file found there is opened. Names that
are not in the map, and files that do not open, are dropped without an error. The archives are
opened in parallel; the result keeps the list's order.

`AHashMap` is `ahash::AHashMap`. A caller without `ahash` in its dependencies can build one by
collecting an iterator of `(NormalizedPath, VfsFile)` pairs straight into the argument, as the
VFS does from its loose files.

{{ api_signature(value="fn file_entries(archives: &ArchiveList) -> Vec<(NormalizedPath, VfsFile)>") }}

Every entry of every archive, archive by archive, in each archive's own order, as a key and a
[`VfsFile`](@/docs/api/files.md). Two entries with the same key are both kept.

{{ api_signature(value="fn file_map(archives: &ArchiveList) -> AHashMap<NormalizedPath, VfsFile>") }}

`file_entries` collected into a map: where two entries share a key, the later one replaces the
earlier, so a later archive wins. Use `file_entries` when every provider matters.

{{ api_signature(value="fn archive_paths(stored: &StoredArchive) -> Vec<PathBuf>") }}

The keys of one open archive, as paths, in the archive's order. Entries that are not safe keys and
ZIP directory entries are skipped.

{{ api_signature(value="type ArchiveList = Vec<Arc<StoredArchive>>") }}

Open archives, in priority order, lowest first.

### StoredArchive

{{ api_signature(value="struct StoredArchive") }}

An open archive and the path it was opened from. `Debug`. Shared through `Arc`: every
[`VfsFile`](@/docs/api/files.md) from it holds a reference, and
[`parent_archive_handle`](@/docs/api/files.md#what-it-is) returns one.

{{ api_signature(value="fn handle(&self) -> &TypedArchive") }}

The open archive.

{{ api_signature(value="fn path(&self) -> &Path") }}

The file it was opened from, as given, not canonicalized.

### TypedArchive

{{ api_signature(value="enum TypedArchive") }}

An open archive, by format. `Debug` prints only the variant. Reading one directly takes the
underlying crate in your own dependencies, at the version vfstool_lib uses.

| Variant | Present | Holds |
|---|---|---|
| `Bethesda(dream_archive::Archive)` | with `beth-archives` | A BSA or BA2 |
| `Zip(Mutex<zip::ZipArchive<File>>)` | with `zip` | A ZIP or PK3, behind a lock so entries are read one at a time |
