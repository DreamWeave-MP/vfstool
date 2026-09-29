+++
title = "Validating and locking"
description = "Check that everything openmw.cfg names exists, write down what every file resolves to, and find out later what changed."
weight = 40

[extra]
kind = "guide"
+++

## Validate the cfg

`validate` checks the cfg's own lines against the disk: every `data=` directory, and every
`fallback-archive=`, `content=` and `groundcover=` file. With nothing wrong it prints an empty
list and exits with 0:

```sh
$ vfstool -c openmw validate
issues: []
```

Here is a cfg with one of each problem: a data directory that is gone, an archive, a plugin and a
groundcover plugin that are nowhere. vfstool lists them in that order and exits with 5:

```sh
$ vfstool -c broken validate
issues:
- !MissingDataDirectory
  path: /tmp/mw/mods/Gone
- !MissingFallbackArchive
  name: Bloodmoon.bsa
- !MissingContentFile
  name: Missing Plugin.esp
- !MissingGroundcoverFile
  name: Grass.esp
```

| Issue | Means |
|---|---|
| `MissingDataDirectory` | A `data=` path does not exist |
| `DataPathNotDirectory` | A `data=` path is a file |
| `MissingFallbackArchive` | No data directory has a file of that name |
| `UnreadableFallbackArchive` | `--full` only: the file is there, but is not an archive vfstool can open |
| `MissingContentFile` | No data directory has the `content=` file |
| `MissingGroundcoverFile` | No data directory has the `groundcover=` file |

Names are matched as the VFS matches them, ignoring case. Plain `validate` looks only at loose
files, which is fast and does not open a single archive. `--full` builds the whole VFS first: it
opens each archive, reports the ones it cannot read, and finds content files that only an archive
has.

```sh
$ vfstool -c broken validate --full
issues:
- !MissingDataDirectory
  path: /tmp/mw/mods/Gone
- !UnreadableFallbackArchive
  name: Bloodmoon.bsa
  path: /tmp/mw/mods/Crisp Textures/Bloodmoon.bsa
- !MissingContentFile
  name: Missing Plugin.esp
- !MissingGroundcoverFile
  name: Grass.esp
```

In JSON, each issue is an object with the issue's name as its one key:
`{"MissingContentFile":{"name":"Missing Plugin.esp"}}`.

## Lock the result

A load order that works is worth writing down. `lock` records every file in the VFS: its key, the
source it comes from, the kind of source, a BLAKE3 hash and the size of the winning copy, and how
many providers it has.

```sh
$ vfstool -c openmw lock -o vfs-lock.yaml
```

The file is sorted by key, so the same VFS always gives the same lock, byte for byte, and it
diffs cleanly in git. Its beginning:

```yaml
schema_version: 1
entries:
- key: icons/m/tx_lantern.dds
  winner_source: /tmp/mw/Morrowind/Data Files/Morrowind.bsa
  winner_kind: archive
  winner_hash_blake3: 4f2145c31041acd2e54ec9785d033493461a062d16b7ec57c6e801fb8611dbe0
  winner_size: 31
  provider_count: 1
- key: lantern glow.esp
  winner_source: /tmp/mw/mods/Lantern Glow
  winner_kind: loose_dir
  winner_hash_blake3: 62702e0ed1e9f9f5a7fbc60acf59d7634dd902f60e98448ad16ee76709380ea2
  winner_size: 17
  provider_count: 1
```

Hashing reads every winning file, archive entries included, so a lock of a large install takes as
long as reading it once.

## Find what changed

`drift` builds the lock again and compares it with a saved one. Nothing changed:

```sh
$ vfstool -c openmw drift vfs-lock.yaml
entries: []
counts: {}
```

After `Crisp Textures` gets a new `tx_wood_01.dds` and a `Meshes/XBase_Anim.nif` of its own:

```sh
$ vfstool -c openmw drift --fail-on-drift vfs-lock.yaml
entries:
- key: meshes/xbase_anim.nif
  kind: winner_source_changed
- key: meshes/xbase_anim.nif
  kind: winner_hash_changed
- key: meshes/xbase_anim.nif
  kind: provider_count_changed
- key: textures/tx_wood_01.dds
  kind: winner_hash_changed
counts:
  winner_source_changed: 1
  winner_hash_changed: 2
  provider_count_changed: 1
```

| Kind | The key |
|---|---|
| `added` | is in the VFS, not in the lock |
| `removed` | is in the lock, not in the VFS |
| `winner_source_changed` | wins from another source than the lock says |
| `winner_hash_changed` | has different content: the hash of its winning copy changed |
| `provider_count_changed` | has more or fewer providers |

One key can drift in several ways, one entry each, sorted by key and then kind. `counts` totals
them by kind. With `--fail-on-drift`, any entry makes vfstool exit with 4 after printing the
report, which is what a script or CI job wants.

`drift` reads the lock as JSON if its name ends in `.json`, TOML if in `.toml`, and YAML
otherwise. A lock whose `schema_version` is not 1, or that does not parse, stops it with exit code
9.
