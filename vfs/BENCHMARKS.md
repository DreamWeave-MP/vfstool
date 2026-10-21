# VFS benchmarks

`bench/bench.jsl` times the package through what it exports: path normalization and ordering
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
  jpk/vfs/bench/bench.jsl <scratch folder> [filter]
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
  leaked (below). Head: `3fe1be6`, the last change to the code: the move as it was made
  (`d629120`) and the three fixes its review made to it (below).
- Protocol: the whole bench (head's file, which base runs unchanged) against base, then head,
  three times over, back to back under one lock; each cell is the median of its three runs. A
  difference of about 5 percent or less is called no change. The table is the review's run. The
  run made when the move was finished, against `d629120`, agrees with it row for row within the
  spread of three rounds on a shared machine (7 to 20 percent between a row's fastest and slowest
  run), which is also all that the rows between 0.94x and 1.12x on code nobody changed are:
  `diffReport`, `keys`, `pathsWith`, `keysStartingWith`, `pathsMatching`,
  `removeResolvedMatchingGlob`.

| Workload | Base | Head | Base / head |
|---|---:|---:|---:|
| paths.normalize | 4.74 ms | 4.81 ms | no change |
| paths.safeKeyFromHost | 8.95 ms | 5.85 ms | 1.53x |
| paths.sortPaths keys | 27.73 ms | 7.32 ms | 3.79x |
| paths.sortPaths absolute | 32.44 ms | 8.84 ms | 3.67x |
| paths.comparePaths | 18.92 ms | 6.31 ms | 3.00x |
| layer.fromFileLists | 20.19 ms | 18.08 ms | 1.12x |
| layer.duplicateKeys | 599.7 µs | 597.6 µs | no change |
| layer.sourceContributions | 1.02 ms | 1.06 ms | no change |
| layer.providerChain | 3.56 ms | 3.13 ms | 1.14x |
| layer.sourcesContaining | 11.10 ms | 8.43 ms | 1.32x |
| conflict.fromFileLists | 37.80 ms | 28.43 ms | 1.33x |
| conflict.conflictsReport relative | 48.58 ms | 14.36 ms | 3.38x |
| conflict.conflictsReport absolute | 68.20 ms | 27.09 ms | 2.52x |
| conflict.shadowedReport | 982.0 µs | 354.9 µs | 2.77x |
| conflict.diffReport | 3.10 ms | 3.31 ms | 0.94x |
| semantic.text | 1.84 ms | 1.86 ms | no change |
| semantic.ini | 4.80 ms | 4.86 ms | no change |
| semantic.json | 6.88 ms | 7.17 ms | no change |
| vfs.fromDirectories loose | 24.73 ms | 22.32 ms | 1.11x |
| vfs.fromDirectories with archive | 40.06 ms | 31.26 ms | 1.28x |
| vfs.pushDirectory | 8.11 ms | 6.49 ms | 1.25x |
| vfs.pushArchive | 17.86 ms | 8.58 ms | 2.08x |
| vfs.getFile | 12.43 ms | 10.22 ms | 1.22x |
| vfs.contains | 8.36 ms | 5.38 ms | 1.55x |
| vfs.providersFor | 2.32 ms | 1.88 ms | 1.23x |
| vfs.explain | 4.08 ms | 3.35 ms | 1.22x |
| file.readAt loose | 2.21 ms | 1.23 ms | 1.80x |
| file.readRange loose | 2.17 ms | 1.36 ms | 1.60x |
| reader.readInto loose | 2.05 ms | 1.25 ms | 1.64x |
| reader.read loose | 2.01 ms | 1.35 ms | 1.49x |
| file.readAt packed | 1.36 ms | 508.5 µs | 2.67x |
| file.readRange packed | 1.32 ms | 605.9 µs | 2.17x |
| reader.readInto packed | 1.15 ms | 528.4 µs | 2.17x |
| reader.read packed | 1.12 ms | 605.0 µs | 1.85x |
| vfs.keys | 29.0 µs | 27.5 µs | 1.05x |
| vfs.entries | 3.26 ms | 3.36 ms | no change |
| vfs.pathsWith | 137.4 µs | 126.6 µs | 1.09x |
| vfs.pathsStartingWith | 75.7 µs | 74.6 µs | no change |
| vfs.keysStartingWith | 21.3 µs | 19.0 µs | 1.12x |
| vfs.pathsMatching | 704.4 µs | 664.4 µs | 1.06x |
| vfs.findByRegex | 17.26 ms | 4.32 ms | 4.00x |
| vfs.tree | 60.47 ms | 8.69 ms | 6.96x |
| vfs.display | 73.28 ms | 15.08 ms | 4.86x |
| vfs.serializeTree json | 77.16 ms | 16.17 ms | 4.77x |
| vfs.duplicates | 10.18 ms | 6.22 ms | 1.64x |
| vfs.serialize duplicates | 70.58 ms | 69.53 ms | no change |
| vfs.archives | 657.2 µs | 638.2 µs | no change |
| vfs.archiveEntries | 87.24 ms | 3.59 ms | 24.28x |
| vfs.filesFromArchive | 87.88 ms | 3.60 ms | 24.40x |
| vfs.diffDirectory | 4.72 ms | 4.38 ms | 1.08x |
| vfs.removeResolvedMatchingGlob none | 2.65 ms | 2.51 ms | 1.05x |
| vfs.removeSource none | 33.36 ms | 3.9 µs | 8514x |
| vfs.removeSource and pushDirectory | 50.25 ms | 8.15 ms | 6.16x |
| vfs.layerIndex rebuilt | 43.39 ms | 7.83 ms | 5.54x |
| vfs.lockManifest | 119.86 ms | 72.13 ms | 1.66x |
| run.snapshotDirectory | 19.77 ms | 15.61 ms | 1.27x |
| run.snapshotDirectoryMetadata | 26.32 ms | 15.54 ms | 1.69x |
| run.changedFiles | 18.91 ms | 15.15 ms | 1.25x |
| vfs.diffAgainstLock | 299.12 ms | 83.92 ms | 3.56x |
| glob.pathGlobMatches | 107.06 ms | 841.4 µs | 127x |

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
encoder's time. `vfs.archives` cannot show its sort, with one archive to list.

Each change made for speed was also timed against the commit before it, three rounds, on the
rows it touches. Two of them do not measure as their subjects read. Finding a lock entry's winner
without its whole chain (`86d685d`) is 1.07x on `lockManifest` and 1.06x on `diffAgainstLock`,
at the edge of what three rounds can tell from nothing. Spelling a provider chain's key once per
chain (`d629120`) is 1.04x on `layer.providerChain` over seven rounds, which is no change; the
1.14x that row shows is no one commit's. Both do strictly less work and answer the same, so they
stay. The three commits that only rewrote loops in the dialect measure as no change on every row
they touch, as they should.

## What the review put back

Nine of the loops the move rewrote as comprehensions walk a table the caller made: the folder
list of `VFS.fromDirectories`, `vfs:remaining` and `ConflictIndex.fromDirectories`, and a layer's
or a conflict index's sources, which sit where `fromFileLists`'s rows did. A comprehension indexes
`1..#list`; the loops it replaced walked whatever was there. So `VFS.fromDirectories { base, nil,
override }`, which had always built a VFS of the two folders, raised from `fs.walk`, and a layer
built from rows with a gap in them stopped listing and counting the source after the gap. Those
nine are loops again (`64d4e0b`, `4993ab0`, `3fe1be6`), each with a case in the suite. Each walks
a handful of sources, so no row's gain was theirs: the builds and the conflict and layer reports,
timed before and after the three fixes over five rounds, show no change.

## A leak the bench found first

The first run of this bench was killed for memory in `vfs.keys`. A key list's view was kept in a
weak table by a function that held the list, and Luau's weak tables are not ephemerons, so every
list `keys()`, `duplicateKeys()` or `filesFromArchive()` ever returned stayed for good. That was
fixed before the baseline was taken (`e1ff176`), so it is in both columns above.

## Which modules are native, and why

Jess compiles to machine code only what is marked (`--!native` on a module, `@native` on one
function), and every load pays for what is marked. A module carries `--!native` when a row below
spends its time there and the module is small (12 KB or less); a larger module carries `@native`
on the functions the row spends its time in, and nothing else.

Measured 2026-10-14 with `jess 0.1.0`, the verified release build of jess `07c38cb` on l3i
2.0.3, on the i7-10870H, the one-minute load 1.18 and 1.33 at the start of the two
sessions. In each session the bench ran alternately on this package and on a copy of it with
every module marked, three fresh runs of `bench/bench.jsl` each, every run's row the median of
its batches; a column is the median of the three, and a ratio is against the all-marked runs of
the same session. Which functions a row spends its time in comes from `jess run --profile` of
that row alone, unmarked. The load cost is a fresh `jess run --package <this package>
--package-path jpk` of a script that only requires `@vfs`, the two sides alternating, the
median of five each after one warm-up.

| Requiring `@vfs` | All marked | This state |
| --- | ---: | ---: |
| Unmarked | 0.180 s | 0.134 s |
| Final | 0.186 s | 0.160 s |

| Row | All marked (ms) | Unmarked / all marked | Final (ms) | Final / all marked |
| --- | ---: | ---: | ---: | ---: |
| paths.normalize | 5.738 | **1.210×** | 5.706 | 0.994× |
| paths.safeKeyFromHost | 7.044 | **1.453×** | 6.785 | 0.963× |
| paths.sortPaths keys | 7.690 | **1.257×** | 7.675 | 0.998× |
| paths.sortPaths absolute | 9.661 | **1.319×** | 9.409 | 0.974× |
| paths.comparePaths | 7.149 | **1.511×** | 6.913 | 0.967× |
| layer.fromFileLists | 18.53 | **1.330×** | 18.82 | 1.016× |
| layer.duplicateKeys | 0.6157 | **1.446×** | 0.6447 | 1.047× |
| layer.sourceContributions | 1.061 | **2.921×** | 1.057 | 0.997× |
| layer.providerChain | 3.386 | **1.316×** | 3.354 | 0.990× |
| layer.sourcesContaining | 9.836 | **1.283×** | 10.09 | 1.026× |
| conflict.fromFileLists | 29.81 | **1.291×** | 30.34 | 1.018× |
| conflict.conflictsReport relative | 14.34 | **1.293×** | 13.94 | 0.972× |
| conflict.conflictsReport absolute | 26.09 | **1.345×** | 26.70 | 1.024× |
| conflict.shadowedReport | 0.3439 | **1.391×** | 0.3330 | 0.968× |
| conflict.diffReport | 3.203 | **1.148×** | 3.190 | 0.996× |
| semantic.text | 1.894 | **1.663×** | 1.938 | 1.023× |
| semantic.ini | 5.115 | **1.675×** | 4.987 | 0.975× |
| semantic.json | 7.186 | **1.559×** | 7.019 | 0.977× |
| vfs.fromDirectories loose | 23.01 | **1.161×** | 23.27 | 1.011× |
| vfs.fromDirectories with archive | 33.42 | **1.166×** | 33.59 | 1.005× |
| vfs.pushDirectory | 6.696 | **1.208×** | 6.830 | 1.020× |
| vfs.pushArchive | 8.999 | **1.300×** | 9.276 | 1.031× |
| vfs.getFile | 11.24 | **1.095×** | 10.86 | 0.966× |
| vfs.contains | 6.305 | **1.212×** | 6.232 | 0.988× |
| vfs.providersFor | 2.148 | **1.226×** | 2.062 | 0.960× |
| vfs.explain | 3.678 | **1.318×** | 3.549 | 0.965× |
| file.readAt loose | 1.431 | **1.708×** | 1.411 | 0.986× |
| file.readRange loose | 1.599 | **1.512×** | 1.570 | 0.982× |
| reader.readInto loose | 1.431 | **1.457×** | 1.422 | 0.994× |
| reader.read loose | 1.556 | **1.363×** | 1.567 | 1.007× |
| file.readAt packed | 0.6576 | **2.487×** | 0.6639 | 1.009× |
| file.readRange packed | 0.8179 | **1.977×** | 0.8216 | 1.005× |
| reader.readInto packed | 0.6552 | **1.864×** | 0.6542 | 0.999× |
| reader.read packed | 0.7784 | **1.687×** | 0.7925 | 1.018× |
| vfs.keys | 0.02803 | **1.067×** | 0.02727 | 0.973× |
| vfs.entries | 3.147 | **1.180×** | 3.315 | **1.053×** |
| vfs.pathsWith | 0.1310 | **1.713×** | 0.1310 | 1.000× |
| vfs.pathsStartingWith | 0.07528 | **1.488×** | 0.07548 | 1.003× |
| vfs.keysStartingWith | 0.01961 | **2.421×** | 0.02193 | **1.118×** |
| vfs.pathsMatching | 0.6824 | **1.344×** | 0.6605 | 0.968× |
| vfs.findByRegex | 4.426 | **1.353×** | 4.293 | 0.970× |
| vfs.tree | 9.010 | **1.386×** | 8.949 | 0.993× |
| vfs.display | 14.57 | **1.240×** | 14.42 | 0.989× |
| vfs.serializeTree json | 17.46 | **1.253×** | 17.12 | 0.981× |
| vfs.duplicates | 6.451 | **1.342×** | 6.229 | 0.965× |
| vfs.serialize duplicates | 72.57 | **1.158×** | 71.06 | 0.979× |
| vfs.archives | 0.6634 | **1.546×** | 0.6627 | 0.999× |
| vfs.archiveEntries | 3.739 | **1.366×** | 3.703 | 0.990× |
| vfs.filesFromArchive | 3.916 | **1.392×** | 3.917 | 1.000× |
| vfs.diffDirectory | 4.561 | **1.150×** | 4.643 | 1.018× |
| vfs.removeResolvedMatchingGlob none | 2.594 | **1.233×** | 2.626 | 1.012× |
| vfs.removeSource none | 0.00417 | **1.533×** | 0.00420 | 1.006× |
| vfs.removeSource and pushDirectory | 8.860 | **1.260×** | 8.946 | 1.010× |
| vfs.layerIndex rebuilt | 8.416 | **1.484×** | 8.468 | 1.006× |
| vfs.lockManifest | 77.52 | **1.134×** | 78.95 | 1.018× |
| run.snapshotDirectory | 16.71 | **1.072×** | 16.36 | 0.979× |
| run.snapshotDirectoryMetadata | 16.78 | **1.079×** | 16.22 | 0.967× |
| run.changedFiles | 16.31 | **1.057×** | 16.18 | 0.992× |
| vfs.diffAgainstLock | 91.09 | **1.198×** | 92.68 | 1.017× |
| glob.pathGlobMatches | 0.9202 | **1.367×** | 0.9410 | 1.023× |

What carries the directive, and the rows that earned it. Every row reads through `paths`, and
the rows are the library's whole surface, so most modules have a function some row spends its
time in:

- Whole: `init` (`pathGlobMatches`), `check` (the argument checks every method makes), `encode`,
  `glob`, `hash`, `json` (the lock manifest), `keys`, `registry`, `run` (snapshots and changed
  files), `semantic` (the text, INI, and JSON comparisons), `state`, `tree`, and `views`.
- `paths`, by function: `normalize`, `normalizeHost`, `isSafeNormalized`, `safeKeyBytes`,
  `safeKeyFromHost`, `atOrUnder`, `lossy`, `keyPath`, `isNames`, `flatten`, `comparePaths`,
  `samePath`, `orderKey`, `sortPaths`, `sortByPath` and its comparator, `join`, `fileName`,
  `parent`.
- `layer`, by function: `stringArgument`, `build`, `sortedKeys`, `chain`, `highest`,
  `lockManifest`, `Methods.sourcesContaining`, `Methods.providerChain`, `Methods.duplicateKeys`,
  `contributions`.
- `conflict`, by function: `fromLayerData`, `reportPath`, `reportList`,
  `Methods.shadowedReport`, `keysOf`, `Methods.diffReport`.
- `file`, by function: `lastComponent`, `copyFrom`, `optionalExact`, `window`, `new`,
  `Methods.readAt`, `Methods.readRange`, `ReaderMethods.read`, `ReaderMethods.readInto`.
- `vfs`, by function: `walkFiles`, `looseSource`, `appendLoose`, `pushBatch`, `entriesWhere`
  and its filter, `record`, `records`, `keyArgument`, `Methods.entries`, `Methods.getFile`,
  `Methods.contains`, `Methods.pathsStartingWith`, `Methods.pathsWith` and their filters,
  `removeMatching`, `Methods.removeSource`, `Methods.removeResolvedMatchingGlob`,
  `Methods.diffDirectory`, `Methods.explain`, `Methods.providersFor`, `Methods.duplicates`,
  `Methods.archives`, `archiveRows`, `Methods.filesFromArchive`, and the closures the profile
  named inside them.
- Unmarked: `host`, `materialize`, and `types`, which no row spends time in.

Marking `paths`, `layer`, `conflict`, and `file` whole instead moved no row past what it is
here, and saved 0.017 s of what requiring `@vfs` costs all marked (0.181 s against 0.198 s, in a
session of its own) where this saves 0.026 s. The two rows above 5% that remain spend their time
in functions that are native already (`vfs.keysStartingWith` is a 20 µs call into `views`'s
`toTable`); what is left is the calls between those and the bytecode around them.
