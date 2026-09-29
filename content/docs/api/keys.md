+++
title = "Keys and paths"
description = "NormalizedPath and the VfsKeyInput trait for VFS keys, normalize_host_path for comparing real paths, the path and source globs, and the SourceId, NormalizedKey and ContentDigest types."
weight = 60

[extra]
kind = "api"
+++

vfstool_lib keeps two kinds of path apart:

- **VFS keys** name files inside the VFS: `textures/tx_wood.dds`. They are bytes, normalized the
  way OpenMW looks files up, and they are [`NormalizedPath`](#normalizedpath) values. Methods that
  take a key take anything that implements [`VfsKeyInput`](#vfskeyinput).
- **Host paths** name real files and directories: data directories, archives, loose files. They
  are `Path` and `PathBuf`, kept as given. When two of them are compared as text, as the CLI does
  to match a data directory, [`normalize_host_path`](#normalize-host-path) folds them first.

```rust
use std::path::Path;

use vfstool_lib::{NormalizedPath, VfsKeyInput, normalize_host_path, path_glob_matches};

fn main() {
    // Any spelling of a key normalizes to the same bytes.
    let key = r"//Meshes\X\Ex_Door.NIF".to_vfs_key();
    assert_eq!(key, NormalizedPath::new("meshes/x/ex_door.nif"));
    assert_eq!(key.extension().unwrap(), "nif");

    // A key that could escape a directory is not safe.
    assert!("../escape.nif".to_safe_vfs_key().is_none());
    assert!("meshes/x/ex_door.nif".to_safe_vfs_key().is_some());

    // Host paths fold, but keep their shape.
    assert_eq!(normalize_host_path(r"C:\Games\Data Files"), Path::new("c:/games/data files"));

    assert!(path_glob_matches("meshes/**/*.nif", Path::new("Meshes/X/Ex_Door.NIF")));
    assert!(!path_glob_matches("meshes/*.nif", Path::new("meshes/x/ex_door.nif")));
}
```

## NormalizedPath

{{ api_signature(value="pub use dream_path::NormalizedPath") }}

The key type, re-exported from [dream-path](https://dreamweave-mp.github.io/dream_path/) 1.0,
which documents it in full. It owns normalized bytes:

- `\` becomes `/`, and ASCII `A` to `Z` become lowercase.
- Repeated separators collapse to one, and leading separators are removed.
- Every other byte stays: bytes that are not UTF-8, non-ASCII letters, `.` and `..`, a trailing
  `/`.

`NormalizedPath::new(bytes)` normalizes; `as_bytes`, `to_str`, `file_name`, `parent`, `extension`,
`len` and `is_empty` read it. It is `Borrow<[u8]>`, which is how a VFS looks keys up by bytes
without building one. The free functions, such as `normalize_path_into`, are in dream-path itself;
depend on `dream-path = "1"` to use them.

## VfsKeyInput

{{ api_signature(value="trait VfsKeyInput: sealed::Sealed") }}

Anything that can be a VFS key. Sealed: it is implemented for `str`, `String`, `Path`, `PathBuf`,
`[u8]` and `NormalizedPath`, and for references to them, and cannot be implemented elsewhere. A
key may be spelled any way; the methods normalize it.

{{ api_signature(value="fn to_vfs_key(&self) -> NormalizedPath") }}

The key, normalized. Nothing is refused: `../a` stays `../a`, a key that no VFS will ever hold.
The provider reports and removals look keys up this way.

{{ api_signature(value="fn vfs_key_bytes(&self) -> Cow<'_, [u8]>") }}

The normalized key's bytes, borrowed when the value is already in normalized spelling: always for
a `NormalizedPath`, and for a string, path or byte slice that is already lowercase, `/`-separated
and without repeated or leading separators. Lookups use this, so `get_file("textures/foo.dds")`
does not allocate.

{{ api_signature(value="fn to_safe_vfs_key(&self) -> Option<NormalizedPath>") }}

The normalized key, or `None` when it could not be written safely under a directory. Methods that
add keys use this. The rules depend on the type:

| Type | Refused when |
|---|---|
| `str`, `String`, `[u8]` | Empty; starting with `/` or `\`; a drive letter (`:` as the second byte); a NUL byte; a `.` or `..` component |
| `Path`, `PathBuf` | Empty after normalization; a `..` component; a root or a Windows prefix; a drive letter. `.` components are dropped: `./a` is `a` |
| `NormalizedPath` | Empty; a drive letter; a NUL byte; a `.` or `..` component. Its leading separators are already gone, so `NormalizedPath::new("/abs")` is the safe key `abs` |

## Host paths

### normalize_host_path

{{ api_signature(value="fn normalize_host_path<P: AsRef<Path> + ?Sized>(path: &P) -> Cow<'_, Path>") }}

A host path folded for textual comparison: `\` becomes `/` and ASCII letters become lowercase.
Nothing else changes: no separators are collapsed or removed, `..` is not resolved, symbolic links
are not followed. Borrowed when the path needs no change. Two spellings of one directory compare
equal after this only if they differ in case and separators alone.

This is not a key normalization; use [`VfsKeyInput`](#vfskeyinput) for keys.

### normalize_host_path_in_place

{{ api_signature(value="fn normalize_host_path_in_place(path: &mut PathBuf)") }}

`normalize_host_path` in place, reusing the `PathBuf`'s allocation. Does nothing when there is
nothing to change.

## Globs

### path_glob_matches

{{ api_signature(value="fn path_glob_matches(glob: &str, path: &Path) -> bool") }}

Whether `path`, a VFS key, matches `glob`. The glob and the path are both folded with
`normalize_host_path` first, so matching ignores ASCII case and treats `\` as `/`. The whole path
must match.

| In the glob | Matches |
|---|---|
| `*` | Any run of characters, `/` excepted, including none |
| `?` | One character that is not `/` |
| `**/` | Zero or more whole directories: `**/foo.txt` matches `foo.txt` and `a/b/foo.txt` |
| `**` elsewhere | Anything, `/` included: `textures/**` matches `textures/a/b.dds`, not `textures` |
| anything else | Itself. `[`, `]`, `{` and `}` have no special meaning |

A glob that cannot be compiled matches nothing.
[`remove_resolved_matching_glob`](@/docs/api/mutation.md#removing) and the
[simulation](@/docs/api/planning.md) buckets and heuristics call it, and the
[experimental](@/docs/api/experimental.md) policies and solver use the same syntax.

### source_glob_matches

{{ api_signature(value="fn source_glob_matches(glob: &str, source_path: &Path) -> bool") }}

The same matching, for a source's host path: `**/mods/*patch*` matches `/home/user/MODS/MyPatch`.
It exists so policy code can say which kind of path it is matching.

## Identifier types

These three are at the crate root and in the `foundation` module.

### SourceId

{{ api_signature(value="struct SourceId(usize)") }}

A source's position in a load order, from 0, lowest priority first, as
[`LayerIndex`](@/docs/api/layer-index.md) numbers its sources. The field is private. `Debug`,
`Clone`, `Copy`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`; `Serialize` and `Deserialize`
with `serialize`.

{{ api_signature(value="const fn from_index(index: usize) -> SourceId") }}

{{ api_signature(value="const fn as_index(self) -> usize") }}

### NormalizedKey

{{ api_signature(value="struct NormalizedKey(NormalizedPath)") }}

A `NormalizedPath` in a wrapper that converts to and from `Path`. The field is private. `Debug`,
`Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Borrow<[u8]>`, and `Display`, which
writes the key as UTF-8 with invalid bytes replaced. With `serialize`, it serializes as that
string, and deserializes from a string, normalizing it. `From<NormalizedPath>`, `From<PathBuf>`
and `From<&Path>`, the last two normalizing. No public function returns one; the index uses it
internally.

{{ api_signature(value="fn new(path: impl AsRef<Path>) -> NormalizedKey") }}

Normalizes `path` into a key. No safety check.

{{ api_signature(value="fn as_path(&self) -> PathBuf") }}

The key as an owned `PathBuf`, despite the name. Byte for byte on Unix; elsewhere, a key that is
not UTF-8 has its invalid bytes replaced.

{{ api_signature(value="fn into_path_buf(self) -> PathBuf") }}

The same, consuming the key.

{{ api_signature(value="fn as_normalized_path(&self) -> &NormalizedPath") }}

The key itself.

### ContentDigest

{{ api_signature(value="struct ContentDigest") }}

A content hash and size. `Debug`, `Clone`, `PartialEq`, `Eq`; with `serialize`, `Serialize` and
`Deserialize<'static>`, which reads from `'static` input only, because `algorithm` is a
`&'static str`. No public function returns one in 1.0: reports carry the hash as a hex string,
such as a [lock](@/docs/api/lock-drift.md) entry's `winner_hash_blake3`.

| Field | Type | Meaning |
|---|---|---|
| `algorithm` | `&'static str` | The algorithm's name: `"blake3"` |
| `hex` | `String` | The digest, lowercase hexadecimal |
| `size` | `u64` | The content's size in bytes |

{{ api_signature(value="fn blake3(bytes: [u8; 32], size: u64) -> ContentDigest") }}

A BLAKE3 digest from its 32 bytes, with a 64-character `hex`.
