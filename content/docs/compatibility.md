+++
title = "Platforms and performance"
description = "The downloads and what they read, building vfstool and vfstool_lib yourself, signed releases, the license and when it changed, what is tested, and what the benchmarks measure."
weight = 80

[extra]
kind = "reference"
+++

## Downloads

| Download | For |
|---|---|
| `vfstool-Windows-X64.zip` | Windows, x86-64 |
| `vfstool-macOS-ARM64.zip` | macOS, Apple silicon |
| `vfstool-macOS-X64.zip` | macOS, Intel |
| `vfstool-Linux-X64.zip` | Linux, x86-64, glibc 2.34 or newer: RHEL 9, Ubuntu 22.04, Debian 12 and later |

Each holds the program and its [signature bundle](#signed-releases). They are the same program:
BSA and BA2 archives, and YAML, JSON and TOML reports, on every platform. None reads ZIP or PK3
archives.

## Building it yourself

```sh
cargo install vfstool
```

builds the program from crates.io with Rust 1.88 or newer. The program has no
features of its own; it always builds `vfstool_lib` with `serialize` and `beth-archives`.

A clone of the repository builds with the toolchain in its `.cargo/config.toml`: clang and lld,
on the same LLVM version as `rustc`, because the library's Luau feature needs them. `cargo install
--path vfstool` from a clone needs them too.

The library's features:

| Feature | Adds |
|---|---|
| `beth-archives` | BSA and BA2, through dream_archive |
| `zip` | ZIP and PK3: stored, deflate and LZMA entries |
| `serialize` | JSON, YAML and TOML, and the `serde`, `serde_json`, `serde_yaml` and `toml` re-exports |
| `lua` | The [`@dream/vfs`](@/docs/luau/_index.md) l3i extension. Needs l3i's toolchain: clang, lld and cross-language thin LTO |
| `luau-analysis` | `lua` plus Luau's analysis frontend, for the typed tests. Not for hosts |

None is on by default; without any, the library reads loose files only.

## Signed releases

Each release archive holds, beside the program, a Sigstore bundle for it, such as
`vfstool-Linux-X64.bundle`, made by StroggForge's release workflow. It proves that workflow built
the program for this repository:

```sh
cosign verify-blob vfstool \
  --bundle vfstool-Linux-X64.bundle \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/DreamWeave-MP/StroggForge/\.github/workflows/rustGlobalBuild\.yml@' \
  --certificate-github-workflow-repository DreamWeave-MP/vfstool
```

Each GitHub release also links every archive's VirusTotal scan.

## License

vfstool and vfstool_lib are MIT OR Apache-2.0, at your option, since 0.10.4. 0.9.0 to 0.10.3
were GPL-3.0-only, and releases before 0.9.0 GPL-3.0-or-later.

## What is tested

Every push runs [StroggForge](https://github.com/DreamWeave-MP/StroggForge)'s release workflow:
the tests with every feature on Windows, Linux, and macOS on Apple silicon and Intel; Clippy at the
pedantic level with warnings as errors; `rustfmt`; and `cargo audit`. Nothing is built for release
until they pass.

The program's tests run the built `vfstool` against small installs they write for themselves: a
cfg of any name through `OPENMW_CONFIG`, quoted paths, TES3 BSAs, the exit codes, locks and
drift, validation, collapse plans, and `run` capturing, refusing and passing through exit codes.
The library's tests cover priority, normalization, keys that are not UTF-8, every mutation,
ZIP and BSA archives, reports, semantic analysis and the solver. The Luau tests type-check the
extension's definitions with Luau's own checker, run a strict script against them, and drive the
module.

## What it costs

`cargo bench -p vfstool_lib --bench vfs_benchmarks` measures the library on synthetic loose data
directories of small files spread over six folders. From the `BENCHMARKS.md` StroggForge attached
to the 0.9.1 release, means on a GitHub Actions runner:

| Operation | Time |
|---|---:|
| Build a VFS from one directory of 100 / 500 / 2000 files | 0.32 / 0.84 / 2.70 ms |
| `get_file`, hit or miss, any spelling, 1000 keys | 47 ns |
| `paths_with`, 1000 keys | 5.1 µs |
| `paths_matching`, 1000 keys | 57 to 61 µs |
| Build a VFS and a `ConflictIndex` together: 1000 + 500 files | 2.83 ms |
| `ConflictIndex` of 1000 + 200 + 200 files with 175 conflicts | 2.30 ms |
| `ConflictIndex` of 50 directories, dense conflicts | 23.4 ms |
| Dump 2000 files, hardlinked / copied | 33.4 / 61.4 ms |
| Serialize a tree as JSON / TOML / YAML | 22.8 / 65.0 / 127.6 µs |

Lookups cost the same for any spelling because the key is normalized first, in a few
nanoseconds.

From Luau, `cargo bench -p vfstool_lib --all-features --bench luau_boundary` runs frozen scripts
against a 1000-file and a 5000-file install, through the `mlua` binding of 0.9.1 to 0.10.4 and
the l3i binding that replaced it, each pinned to one core; the minimum of five runs on a loaded machine, per call:

| Script | mlua 0.12 | l3i |
|---|---:|---:|
| `getFile` hit (`textures/file_00006.dat`) | 400 ns | 250 ns |
| `getFile` hit, spelled `Textures\File_00006.DAT` | 396 ns | 286 ns |
| `getFile` miss | 167 ns | 98 ns |
| `contains` hit / miss | 181 / 185 ns | 91 / 88 ns |
| `len`, 5000 keys | 82 ns | 53 ns |
| `keys`, 5000 keys | 1.33 ms, a table | 618 µs the first call, 279 ns after, a view; `:toTable()` 359 µs |
| `entries`, 1000 | 823 µs, a table | 226 µs, a view; `:toTable()` 620 µs |
| `pathsMatching`, 1000 hits / 1 hit | 1.03 ms / 79 µs | 262 µs / 65 µs |
| `pathsWith('textures')`, 167 hits | 141 µs | 41 µs |
| `providersFor` hit | 1.58 µs | 341 ns |
| `explain` hit | 2.84 µs | 1.99 µs |
| `tree`, 1000 files | 2.11 ms, nested tables | 898 µs, a stream; `:toTable()` 1.47 ms |
| `file:readAll()`, 64 KiB | 19.6 µs | 19.0 µs |
| `file:readInto(buffer)`, 64 KiB | none | 5.4 µs |
| `file:path()` | 164 ns | 105 ns |

The views are what make key lists cheap: a script that measures, indexes or walks them never
copies the collection into a table.
