# VFS benchmarks

`bench/bench.luau` times the package through what it exports: path normalization and ordering
(`@vfs/paths`), layer and conflict indexes over a synthetic load order (nine sources, 20,400
files, 10,158 keys), semantic comparison, and a VFS over four folders of 1,500 files and a
3,666-entry BSA (7,124 keys) written under a scratch folder: built, looked up, read from, listed,
reported on, mutated, locked, and snapshotted. Each workload is warmed up, sized to a batch of
about a tenth of a second, and reported as the median of seven batches, with a full collection
between them. Every workload answers a count, a length or a kind that the bench works out from
its own fixture without the port, so a path that stopped doing the work fails instead of posting
a time.

```sh
jess run --package jpk/vfs --package-path jpk \
  --capability filesystem.read --capability filesystem.write \
  jpk/vfs/bench/bench.luau <scratch folder> [filter]
```

One line per workload: its name, a tab, the seconds one call takes. A workload's call is the
whole loop its name stands for: `vfs.getFile` is 20,400 lookups by every spelling the load order
holds, `file.readAt` is 4,096 reads of 16 bytes, `paths.sortPaths keys` is one sort of 10,158
keys.

## The move onto l3i 2.0, base against head

- Machine: Intel Core i7-10870H @ 2.20 GHz, 16 threads, shared with other work while this ran.
- Date: 2026-10-12.
- Binary: the verified release build of Jess 0.1.0 on l3i 2.0.1 (Luau 0.742), the same one for
  both sides.
- Base: `933af6a`, the package before the move, with this bench and with its key lists no longer
  leaked (below). Head: `d629120`, the last change to the code.
- Protocol: the whole bench (head's file, which base runs unchanged) against base, then head,
  three times over, back to back under one lock; each cell is the median of its three runs. A
  difference of about 5 percent or less is called no change.

| Workload | Base | Head | Base / head |
|---|---:|---:|---:|
| paths.normalize | 4.86 ms | 4.78 ms | no change |
| paths.safeKeyFromHost | 8.99 ms | 5.76 ms | 1.56x |
| paths.sortPaths keys | 27.82 ms | 7.20 ms | 3.86x |
| paths.sortPaths absolute | 33.53 ms | 8.71 ms | 3.85x |
| paths.comparePaths | 19.33 ms | 6.31 ms | 3.07x |
| layer.fromFileLists | 20.78 ms | 17.56 ms | 1.18x |
| layer.duplicateKeys | 612.1 µs | 591.1 µs | no change |
| layer.sourceContributions | 1.04 ms | 1.02 ms | no change |
| layer.providerChain | 3.68 ms | 3.16 ms | 1.17x |
| layer.sourcesContaining | 11.51 ms | 8.20 ms | 1.40x |
| conflict.fromFileLists | 38.27 ms | 27.72 ms | 1.38x |
| conflict.conflictsReport relative | 51.48 ms | 13.43 ms | 3.83x |
| conflict.conflictsReport absolute | 71.04 ms | 24.52 ms | 2.90x |
| conflict.shadowedReport | 1.02 ms | 326.4 µs | 3.12x |
| conflict.diffReport | 3.20 ms | 3.10 ms | no change |
| semantic.text | 1.85 ms | 1.82 ms | no change |
| semantic.ini | 4.98 ms | 4.84 ms | no change |
| semantic.json | 7.12 ms | 6.92 ms | no change |
| vfs.fromDirectories loose | 24.85 ms | 21.88 ms | 1.14x |
| vfs.fromDirectories with archive | 39.54 ms | 32.19 ms | 1.23x |
| vfs.pushDirectory | 8.01 ms | 6.51 ms | 1.23x |
| vfs.pushArchive | 17.55 ms | 8.61 ms | 2.04x |
| vfs.getFile | 12.21 ms | 9.96 ms | 1.23x |
| vfs.contains | 8.08 ms | 5.45 ms | 1.48x |
| vfs.providersFor | 2.25 ms | 1.88 ms | 1.19x |
| vfs.explain | 3.98 ms | 3.48 ms | 1.14x |
| file.readAt loose | 2.22 ms | 1.29 ms | 1.72x |
| file.readRange loose | 2.16 ms | 1.40 ms | 1.55x |
| reader.readInto loose | 2.07 ms | 1.31 ms | 1.58x |
| reader.read loose | 2.00 ms | 1.42 ms | 1.41x |
| file.readAt packed | 1.36 ms | 510.1 µs | 2.67x |
| file.readRange packed | 1.33 ms | 597.1 µs | 2.22x |
| reader.readInto packed | 1.20 ms | 549.1 µs | 2.19x |
| reader.read packed | 1.22 ms | 624.7 µs | 1.95x |
| vfs.keys | 28.8 µs | 26.4 µs | 1.09x |
| vfs.entries | 3.26 ms | 3.27 ms | no change |
| vfs.pathsWith | 132.9 µs | 126.6 µs | no change |
| vfs.pathsStartingWith | 74.2 µs | 73.6 µs | no change |
| vfs.keysStartingWith | 19.6 µs | 19.0 µs | no change |
| vfs.pathsMatching | 673.5 µs | 649.4 µs | no change |
| vfs.findByRegex | 16.53 ms | 4.17 ms | 3.96x |
| vfs.tree | 56.96 ms | 8.00 ms | 7.12x |
| vfs.display | 75.86 ms | 13.19 ms | 5.75x |
| vfs.serializeTree json | 79.65 ms | 15.77 ms | 5.05x |
| vfs.duplicates | 10.73 ms | 6.17 ms | 1.74x |
| vfs.serialize duplicates | 73.32 ms | 69.73 ms | no change |
| vfs.archives | 649.8 µs | 645.9 µs | no change |
| vfs.archiveEntries | 87.35 ms | 3.57 ms | 24.49x |
| vfs.filesFromArchive | 87.22 ms | 3.62 ms | 24.07x |
| vfs.diffDirectory | 4.73 ms | 4.37 ms | 1.08x |
| vfs.removeResolvedMatchingGlob none | 2.63 ms | 2.54 ms | no change |
| vfs.removeSource none | 35.04 ms | 4.0 µs | 8802x |
| vfs.removeSource and pushDirectory | 52.27 ms | 8.30 ms | 6.30x |
| vfs.layerIndex rebuilt | 45.26 ms | 7.76 ms | 5.83x |
| vfs.lockManifest | 122.63 ms | 74.77 ms | 1.64x |
| run.snapshotDirectory | 19.81 ms | 16.41 ms | 1.21x |
| run.snapshotDirectoryMetadata | 26.47 ms | 16.28 ms | 1.63x |
| run.changedFiles | 19.20 ms | 15.95 ms | 1.20x |
| vfs.diffAgainstLock | 317.53 ms | 87.90 ms | 3.61x |
| glob.pathGlobMatches | 108.73 ms | 802.6 µs | 135x |

## What moved, and why

Nothing here is the dialect's doing as syntax. The package has no numeric buffers, so the data
plane had nothing to take, and the forty or so loops that became comprehensions, reducers, sinks,
slices and patterns run as the loops they replaced. What moved is work that stopped being done.

- **Sorting by path** (`sortPaths`, the conflict reports, `duplicates`, the lock manifest;
  `archiveEntries`, `filesFromArchive`, `archives`, lock drift). `sortPaths` spelled every path's
  order key by splitting it into components, then sorted through a comparator that looked both
  keys up; the row sorts called `comparePaths`, which split both sides of every comparison, up
  to three times a comparison. A path of plain names is now spelled with one `gsub` and never
  split, and where every key is one path's alone the keys go to `table.sort` with no comparator
  at all. Ties still go through a comparator, over the keys, so the order of rows that tie is
  the one it was. That is the 24x on archive listings and most of the 3.6x on drift.
- **The tree** (`tree`, `display`, `serializeTree`, `findByRegex`). It walked from the root to a
  file's folder for every file, comparing each step against the root's name by components, and
  sorted each folder through a comparator. It now walks once per folder, takes a file's name and
  parent from its last separator (`bytes.rfind`), and sorts names as plain strings.
- **Reads** (`file.*`, `reader.*`). A sixteen-byte read spent half its time formatting the
  method's name for an error it was not going to raise, and calling into the argument checks
  four or five times. The names are spelled once, and a whole number or a buffer no longer
  leaves the module to be told it is one. The copy itself was never the cost.
- **Lookups** (`getFile`, `contains`, `providersFor`, the layer's `sourcesContaining`): the same
  name formatting, gone. What is left is the intern pool's lookup and the handle a file is given.
- **Building** (`fromDirectories`, `pushDirectory`, `pushArchive`, the index builds). A key was
  normalized twice and its components walked twice; a new key asked for its token twice and
  counted itself into every folder above it. Each happens once now, and a folder counts what sits
  directly in it. The walk of the disk is about 40 percent of a build and is the host's.
- **Hashing** (`lockManifest`, `diffAgainstLock`, the `run.*` snapshots). Every file was asked
  whether it exists, mapped, read, and unmapped. A regular file that fits one read is described
  once and read once; larger ones still go through a reader in pieces.
- **Removing a source** went through `comparePaths` for every provider of every key and
  refreshed every stack. It compares once per source and refreshes the stacks that changed, and
  a path no source has ends there, which is the absurd ratio in its row.
- **Globs**: `pathGlobMatches` compiled its regular expression on every call. It keeps the last.

## What did not

`paths.normalize` is one `bytes.translate` call and stays one. The semantic comparisons, the
prefix and substring listings, `entries` (a file handle per row), `archives` and `diffReport`
were not changed in substance and do not move. `serialize duplicates` is mostly the built-in
encoder's time. `vfs.keys` shows 9 percent on a 27 µs copy that was not touched: read it as noise.

## A leak the bench found first

The first run of this bench was killed for memory in `vfs.keys`. A key list's view was kept in a
weak table by a function that held the list, and Luau's weak tables are not ephemerons, so every
list `keys()`, `duplicateKeys()` or `filesFromArchive()` ever returned stayed for good. That was
fixed before the baseline was taken (`e1ff176`), so it is in both columns above.
