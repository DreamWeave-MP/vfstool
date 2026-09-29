# Notes for whoever works on vfstool next

Found on 2026-09-29 while writing the documentation site against the source, at 1.0.0 (fbf0425).
Every bug found then is fixed; what is left needs a decision from the maintainer, or belongs to
other work.

## Needs a decision before 1.0.0 is tagged

- **The library writes to standard error**, which a host can neither capture nor silence:
  `collapse_into` for each archive entry or archive it skips, `dump_to_directory` for each file it
  skips, and `diff_directory` for each entry its walk cannot read (`eprintln!` in
  `vfs/materialize.rs` and `vfs/diff.rs`). The site documents it (`docs/api/_index.md`, "Standard
  error"). Fixing it changes public API: return the skipped keys (a report type from
  `collapse_into` and `dump_to_directory`), take a callback, or log through `log`, and have the
  program print what it gets back. Which one is the maintainer's call, and 1.0.0 is the last
  chance to make it without a major version.
- **A usage error exits with clap's 2**, the code `find-file -p` uses for "only in an archive".
  `cli.md` documents both under 2. Mapping clap's errors to 8 ("invalid input") would separate
  them, but changes a documented exit code.
- **Global options only work before the command** (`vfstool -c DIR find-file x`). clap's
  `global = true` would let them go anywhere, but `-r` is both the global `--use-relative` and
  `remaining --replacements-only`, so one of the two needs another short name first.
- **The released program is built without the library's `zip` feature**, so it does not read ZIP,
  PK3 or JPK archives even though the library can. OpenMW's own VFS reads BSA and BA2; whether the
  program should go further is a product decision.

## Loose ends

- `vfstool_lib` depends on `dream-path` and `l3i` by path (`../../dream-path`,
  `../../dream-binder`). CI has no sibling checkouts, so every job fails at dependency resolution
  until they are crates.io versions again; `AGENTS.md` plans that for the release.
- `.gitlab-ci.yml` is the April 2025 GitLab pipeline, still building the old `vfstool-linux.zip`
  layout. Delete it unless a GitLab mirror still runs it.
- The site does not know about JPK archives and Zstandard ZIP entries yet (57499f4, 79a4805): it
  names ZIP and PK3 only, and `docs/api/archives.md` still says zstd entries do not read. The
  rustdoc was updated with the feature (9d2c51e).
