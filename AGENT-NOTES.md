# Notes for whoever works on vfstool next

Found on 2026-09-29 while writing the documentation site against the source, at 1.0.0 (fbf0425).
The repository was in flux, so none of this was fixed; the site describes what the code does
today, and names the problems where a reader would meet them. Each was reproduced.

## Bugs that lose data

- **`collapse` into one of the load order's data directories deletes that directory's files.**
  `collapse_loose_file` (`vfstool_lib/src/vfs/materialize.rs:187`) removes the destination before
  linking; when the destination is the source, the source is gone and the link, and the `-a`
  copy, fail. Reproduce: a cfg with `data="/tmp/x/Data"` holding `textures/a.dds`, then
  `vfstool -c <cfg dir> collapse -a /tmp/x/Data`: exit 9, `link failed ... also failed: No such
  file or directory`, and `a.dds` is deleted. The README's old Oblivion example
  (`collapse -ae C:\Games\Oblivion\Data` with that folder in the cfg) does exactly this.
- **`extract` into the folder a loose file already lives in deletes it.** `extract_file` →
  `copy_replacing_output` (`materialize.rs:327`) removes the destination, then copies from it.
  Reproduce: `vfstool extract textures/a.dds /tmp/x/Data/textures`: exit 9, file deleted.

## Bugs

- **Reports with a non-UTF-8 key cannot be serialized (a 1.0.0 regression).** Since f87636e,
  `key_to_path_buf` (`vfstool_lib/src/paths.rs:156`) builds a `PathBuf` from the key's raw bytes on
  Unix, and serde refuses to serialize a `PathBuf` that is not UTF-8. `vfstool explain`,
  `duplicates`, `conflicts`, `shadowed -l`, `lock`, `drift` and `collapse --dry-run` all stop with
  `vfstool: path contains invalid UTF-8 characters`, exit 9, when any key in the VFS has a non-UTF-8
  name. 0.10.4 converted keys lossily (`key_to_path_buf_lossy`), so it wrote U+FFFD instead.
  Reproduce: a data directory with `Textures/caf\xe9.dds`, then `vfstool lock`. `find`, `diff` and
  `contributions` still work. The library's own reports are fine until they are serialized.
- **Archive entries are named by their whole in-archive path on Linux and macOS.**
  `VfsFile::file_name` and `file_stem` (`vfstool_lib/src/vfs_file.rs:266-304`) call
  `Path::file_name` on the stored entry name, and TES3 BSAs store `meshes\x\ex_door.nif` with
  backslashes, which are not separators there. `vfstool find`, `remaining`, `VFS::tree` and
  `Display` list `meshes\x\ex_door.nif` as the file name, and sort by it; Luau `file:fileName()`
  too. Reproduce: any Morrowind BSA in `fallback-archive=`, then `vfstool find nif`.
- **`archives -f toml` and `archive-list -f toml` always fail** with `vfstool: unsupported array
  type`, exit 9: both reports are bare arrays (`vfstool/src/commands.rs:297` and `:308`), and TOML
  has no top-level arrays. Wrap them in a table, or refuse `toml` for them.
- **`serialize` in Luau rejects `Entries` and `Providers` views**, and tree streams, with
  `dream.vfs: serialize: userdata other than a sequence view cannot be serialized`: their rows
  hold `VfsFile` userdata, which `convert` refuses (`vfstool_lib/src/lua/serialize.rs`, the
  `Type::Userdata` branch and the `sequence_items` probes). `lua.md` says views serialize as their
  `toTable()` rows; only `Keys` and `ProviderRecords` do. Reproduce:
  `vfstool.serialize(vfs:entries(), "json")`.
- **The text display of a VFS loses the folder hierarchy.** `write_node` and `write_tree`
  (`vfstool_lib/src/vfs/tree.rs`) print each folder by its last component only, all at one level,
  and skip folders that hold only folders: `meshes/x/ex_door.nif` shows under `├── x/`, and an
  archive's `Extras.zip/textures` looks like the loose `textures/`. This is `impl Display for VFS`,
  `display_filtered` and Luau `vfs:display()`.
- **`diff` with a data directory spelled in another case reports it as empty.**
  `validate_configured_data_path` (`vfstool/src/commands.rs:244`, called from `handle_diff` at :731)
  accepts the spelling after folding ASCII case and slashes, but `ConflictIndex::diff_report`
  (`vfstool_lib/src/conflict/diff.rs:11-35`) walks the path as typed and finds its position by exact
  comparison. On a case-sensitive file system, `vfstool diff '/mods/crisp textures' '/mods/Lantern
  Glow'` (the cfg says `Crisp Textures`) prints `only_in_a: []`, and `higher_priority` falls back to
  `source_b`, which is wrong. `remaining` does not have this problem. More generally,
  `ConflictIndex::diff_report` ignores the index it is called on and walks both paths from disk
  again, so an archive source compares as empty, and an index built from file lists or a
  `LayerIndex` is not what it reports on.
- **Absolute paths in `conflicts` and `shadowed -l` use the lowercase key.** `report_path`
  (`vfstool_lib/src/conflict/reports.rs:93-100`) joins the source path with the normalized key:
  `a/Textures/Rock.dds` is reported as `a/textures/rock.dds`, a path that does not exist on a
  case-sensitive file system. The rustdoc of `conflicts_report` promises absolute paths.
- **`source_index` means two things.** `LayerIndex::source_contributions` numbers only sources that
  provide a file (`VFS::build_layer_index` compacts them, `vfstool_lib/src/vfs/core.rs:153-161`);
  `VfsProviderRecord` (`explain`, `duplicates`) and `ArchiveInfo` count every source. With an empty
  first data directory, the second one is 1 in `explain` and 0 in `contributions`.
- **Simulations miscount winners when a source provides no files.** `current_winner_source_idx`
  (`vfstool_lib/src/analysis/simulate.rs:200-207`) compares a VFS source position with a
  `LayerIndex` position, and the compaction above makes them differ. Reproduce:
  `VFS::from_directories_with_layer_index([empty, a, b], None)`, then
  `layer.simulate(&vfs, ReorderOp::FullOrder(<the same order>))` reports `changed_winners == 2`, not
  0, and every `wins_before` is 0. `plan_candidate_directory` returns an empty
  `current_winner_source` (`analysis/candidate.rs:47`) and `simulate_impact` inflates its counts for
  the same reason; a source emptied by `remove_source` triggers it too.
- **Simulations let an archive beat a loose file.** `winner_after_reorder`
  (`analysis/simulate.rs:209`) takes the highest-ranked source of any kind; the VFS never does, and
  neither does the solver's `winner_for_providers` (`solve/evaluate.rs:211`). Reproduce with the
  `zip` feature: `base.zip` holding `textures/rock.dds` in one data directory, a loose directory `b`
  with the same file, then `simulate(&vfs, ReorderOp::MoveAfter { source: <zip>, after: b })`
  reports `changed_winners == 1`.
- **`set_winner_file` allows a file and a directory with one name.** On a new key it does not
  record the key's parent directories (`vfstool_lib/src/vfs/mutate.rs:20-47`, the insert at :41
  has no `add_dir_prefixes`), so `set_winner_file("a/b.txt", f)` then `set_winner_file("a", g)`
  keeps both, and the VFS is no longer materializable.
- **Removing a whole stack leaves stale directory counts.** After `push_provider("a/b.txt", p)` and
  `remove_resolved_file("a/b.txt")`, `push_provider("a", …)` returns false. `remove_resolved_prefix`
  behaves the same; `remove_winner` does not. `mutate.rs:233` and `:338`.
- **`push_provider_batch` does not check its entries against each other**
  (`mutate.rs:93-135`, the check at :110). A batch with `a` and `a/b.txt` adds both, and
  `push_directory` and `push_archive` go through it, so a directory holding a file `A` and
  `a/b.txt` gives both keys, where `from_directories` keeps only one.
- **`display_filtered` and `impl Display for VFS` panic** on a file whose path has no file name,
  such as `VfsFile::from("/")`: `vfstool_lib/src/vfs/tree.rs:158` unwraps `file_name()`.
- **Possibly: symbolic-link collapses of relative data directories make broken links.**
  `collapse_into` with `use_symlinks` links to `file.path()` as stored (`materialize.rs:202`), so
  a relative source path does not resolve from the output folder. The program always passes the
  absolute paths openmw-config resolves, so only library callers can hit it.

- **Minor, in the Luau binding:** `__tostring` is registered twice for `VFS`, in `describe` and in
  `describe_archives_and_plans` (`lua/vfs.rs`); an unknown source kind gives a doubled prefix,
  `VfsProvider.new.kind: dream.vfs: unknown source kind 'zip' (...)`, because
  `source_kind_from_name` (`lua/mod.rs`) adds `dream.vfs:` inside the option reader's context; and
  `lua::Keys(pub KeyBlob)` cannot be filled by a host, since `KeyBlob` sits in the private `views`
  module and is not re-exported.
- **The library writes to standard error**, which a host can neither capture nor silence: collapse
  skipping archive files and entries, the dump skipping missing sources, and `diff_directory`'s
  walk warnings (`eprintln!` in `vfs/materialize.rs`, four calls, and `vfs/diff.rs`, one).

## Docs that disagree with the code

The site follows the code. These comments and docs, in files this work did not change, do not:

- `vfstool_lib/src/vfs/materialize.rs:20`, `dump_to_directory`: "The destination directory must
  already exist." It creates it.
- `vfstool_lib/src/vfs/tree.rs:118`, `display_filtered`: "Panics only if formatting a `String`
  fails." See the panic above.
- `vfstool_lib/src/directory_node.rs:62`, `DirectoryNode::filter`: the predicate takes
  `&VfsFile`, not `&Arc<VfsFile>`.
- `VfsFile::file_name` and `file_stem`: they return `Option<&OsStr>`, not `Some(&str)`.
- `vfstool_lib/src/vfs/mutate.rs:273`, `remove_provider_prefix`: it does not reveal lower
  providers; it removes every provider of every matching key.
- `SourceMeta.path` is documented as an absolute path; it is whatever path was passed in.
- `SemanticConflict.asset_class`, "Inferred asset class", stays `Unknown` unless
  `include_semantic_deltas` is set.
- `CandidatePlanOpts.include_semantic` and `CandidateConflict.semantic_differs` compare raw BLAKE3
  hashes, not the semantic analysis; `CandidateConflict.risk` is always `None`.
- `ProviderRecord.hash_blake3` says hashes are unavailable for some archive providers;
  `provenance(.., true)` hashes every one, and only a loose file that no longer exists has none.
- `ConflictIndex::from_directories_with_archives` says "BSA/BA2 archive files"; with `zip` it takes
  ZIP and PK3 too.
- `vfstool_lib/src/vfs/providers.rs`: `UnsafeDestination` is documented as reserved, but
  `materialization_plan` emits it when the output root or a folder on the way is a symbolic link;
  `Copy` and `FileDirectoryConflict` are never emitted.
- `vfstool_lib/src/lua/mod.rs` rustdoc lists only `VFS` and `VfsFile` as tagged; `Keys` and
  `Entries` are tagged too.
- `vfstool_lib/docs/lua.md`: the install line says `version = "0.11"` (the crate is 1.0.0); it
  says views serialize (see above); and it warns that `runSetup` hardlinks but not that it deletes
  `mergedDir` recursively first. The command line refuses a non-empty folder; the library and Luau
  do not.
- `AGENTS.md` lists the exit codes without 5, which `validate` uses.

The old READMEs had two more, fixed with the move to the site: the library README called
`vfs.serialize_from_tree(...)` as a method (it is an associated function), and the program README
suggested collapsing into the game's own `Data` folder (see the first bug).

## Loose ends

- `vfstool_lib` depends on `dream-path` and `l3i` by path (`../../dream-path`,
  `../../dream-binder`). CI has no sibling checkouts, so every job fails at dependency resolution
  until they are crates.io versions again.
- There is no `--version`. The 0.1.9 and 0.9.1 releases were built from a `Cargo.toml` that still
  said 0.1.8 and 0.9.0, which nothing in the program would show.
- Colors are written even when the output is not a terminal, and `NO_COLOR` is ignored; only
  `find-file -s` is plain.
- Global options only work before the command (`vfstool -c DIR find-file x`); clap's `global =
  true` would let them go anywhere, and would also free `-r` from meaning two things.
- A usage error exits with clap's 2, the code `find-file -p` uses for "only in an archive".
- The released program is built without the library's `zip` feature, so it does not read ZIP or
  PK3 archives even though the library can.
- `.gitlab-ci.yml` is the April 2025 GitLab pipeline, still building the old `vfstool-linux.zip`
  layout.
