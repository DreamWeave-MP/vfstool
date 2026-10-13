# vfstool benchmarks

Three benches, because a command-line tool has three costs.

`bench/bench.luau` times the commands warm, through the package's one export, `run`: each over a
synthetic mod list written under a scratch folder (240 data folders of 25 files, 200 plugins and
6 groundcover plugins beside them, and a 2,500-entry BSA: 3,816 keys), its report written to a
file. One line per workload: its name, a tab, the seconds one run of the command takes with the
configuration, the VFS and the report included. Each is warmed up, sized to a batch of about a
tenth of a second, and reported as the median of seven batches with a full collection between
them. Every run's exit code is checked, and its report is held to a count the mod list's
generator works out without the port. The reports stay in `<scratch>/vfstool-bench/reports`: two
builds run over the same scratch folder must leave the same bytes there.

`bench/cold.luau` times what one process pays once, from inside it: `@vfstool` required, then the
first `validate`, `lock` and `drift` over three folders, each of which compiles what it is the
first to need. With `own`, every module of another package is loaded first, so the lines time
this package's own modules alone. Each line is one sample, so it is run many times and the
medians are compared.

`bench/process.luau` times whole processes from outside: `jess package run` of each package
folder it is given, turn and turn about, for six commands over three folders, and for two more
over `bench.luau`'s mod list when that has been left in the same scratch folder.

```sh
jess run --package jpk/vfstool --package-path jpk \
  --capability filesystem.read --capability filesystem.write \
  jpk/vfstool/bench/bench.luau <scratch folder> [filter]

jess run --package jpk/vfstool --package-path jpk \
  --capability filesystem.read --capability filesystem.write \
  jpk/vfstool/bench/cold.luau <scratch folder> [own]

jess run --capability filesystem.read --capability filesystem.write --capability process.spawn \
  jpk/vfstool/bench/process.luau <jess> <package path> <scratch folder> <rounds> <package folder>...
```

## The move onto l3i 2.0, base against head

- Machine: Intel Core i7-10870H @ 2.20 GHz, 16 threads, shared with other work while this ran.
- Date: 2026-10-12, the runs ending after midnight.
- Binary: the verified release build of Jess 0.1.0 on l3i 2.0.2 (Luau 0.742), the same one for
  both sides.
- Base: `0dda493`, the package before the move, with the reports suite and the benches. Head:
  the source of `5864b68`, which no later commit changes.
- Protocol: every A/B alternates base and head, back to back, under one lock. `bench.luau`: three
  runs of each, each cell the median of its three. `cold.luau`: thirty-one runs of each, in each
  mode. `process.luau`: twenty-one rounds. A difference under 5 percent is called no change.

### Warm: one run of a command, everything loaded

| Workload | Base, ms | Head, ms | Base / head |
|---|---:|---:|---:|
| validate | 344.37 | 7.59 | 45.4x |
| validate --full | 112.39 | 111.63 | no change |
| find-file -s, a file nothing has | 119.77 | 121.42 | no change |
| explain | 108.79 | 110.47 | no change |
| archives | 110.88 | 110.17 | no change |
| find -r | 110.63 | 112.34 | no change |
| remaining -r | 138.35 | 141.90 | no change |
| duplicates | 230.61 | 232.35 | no change |
| archive-list | 133.97 | 133.78 | no change |
| contributions | 120.98 | 121.16 | no change |
| conflicts | 169.76 | 172.63 | no change |
| conflicts -r | 140.51 | 142.49 | no change |
| shadowed -l | 122.32 | 123.06 | no change |
| diff | 124.32 | 123.44 | no change |
| collapse --dry-run, json | 207.97 | 206.91 | no change |
| collapse --dry-run, yaml | 514.07 | 518.66 | no change |
| lock, json | 203.22 | 205.21 | no change |
| lock, yaml | 553.50 | 560.52 | no change |
| lock, toml | 263.26 | 263.89 | no change |
| drift from a json lock, none | 202.98 | 204.21 | no change |
| drift from a yaml lock, none | 411.82 | 408.64 | no change |
| drift from a toml lock, none | 188.17 | 186.30 | no change |
| drift from a json lock, forty folders added | 212.21 | 213.26 | no change |

The twenty-two reports the two sides left were the same bytes.

### Whole processes, timed from outside

| Command | Base, ms | Head, ms | Base / head |
|---|---:|---:|---:|
| help | 223.15 | 74.07 | 3.01x |
| find-file -s, a loose file | 223.25 | 73.81 | 3.02x |
| find-file -s, a file nothing has | 223.61 | 205.80 | 1.09x |
| validate | 224.08 | 89.08 | 2.52x |
| lock | 223.94 | 208.31 | 1.08x |
| drift, from a JSON lock | 225.29 | 211.35 | 1.07x |
| find-file -s, the first of 240 folders | 229.26 | 79.75 | 2.87x |
| validate, 240 folders | 568.64 | 96.88 | 5.87x |

The same six commands at each commit made for speed, in one run of twenty-one rounds, in ms:

| Command | `0dda493` | `a9e8c68` | `c354e37` | `c060ba5` | `93b3475` |
|---|---:|---:|---:|---:|---:|
| help | 224.22 | 225.09 | 88.75 | 90.10 | 74.76 |
| find-file -s, a loose file | 224.18 | 225.53 | 90.35 | 88.33 | 74.32 |
| find-file -s, a file nothing has | 226.10 | 224.30 | 206.52 | 207.61 | 206.47 |
| validate | 225.46 | 224.86 | 89.17 | 89.61 | 89.22 |
| lock | 224.55 | 226.32 | 208.29 | 209.92 | 207.08 |
| drift, from a JSON lock | 225.43 | 224.11 | 225.17 | 211.59 | 212.68 |

### One cold process, from inside

| One cold process | Base, ms | Head, ms | Base / head |
|---|---:|---:|---:|
| require @vfstool | 177.22 | 24.67 | 7.18x |
| first validate | 0.28 | 15.16 | pays for the reports |
| first lock | 0.84 | 120.70 | pays for `@vfs` |
| first drift | 0.83 | 3.76 | pays for the lock reader |
| all four | 179.11 | 164.58 | 1.09x |
| require @vfstool, other packages loaded | 13.07 | 10.29 | 1.27x |
| first validate, other packages loaded | 0.29 | 2.77 | pays for `reports.luau` |
| first lock, other packages loaded | 0.82 | 0.86 | no change |
| first drift, other packages loaded | 0.76 | 1.50 | pays for `lock.luau` |
| all four, other packages loaded | 14.97 | 15.52 | no change |

### What moved, and why

Two things, and neither is the dialect.

**`validate` listed every data folder again for every file it looked for.** Without `--full` it
finds each fallback archive, content file and groundcover file among the loose files, walking the
data folders from the last to the first, and each step of each walk read a folder from the disk
and folded every name in it. Over 240 folders and 207 files that is some 25,000 listings. A
command now lists a folder once, keyed by the spelling of each name as a VFS key, and a folder
with nothing of a path's first name costs one lookup (`a9e8c68`): 344 ms to 7.6 ms, and what is
left is reading 240 folders once. The folders are read at most once each where they were read
again for every file, so a folder that changes while `validate` runs is now seen as it was when
first read. `find-file` takes the same road for its one path and is the same speed.

**Every command compiled everything.** `@vfs` with the archive package under it takes about
120 ms of a cold process to compile, the report shapes and the three writers under them about 15,
the YAML reader 14, and each was required when the package was, whatever the command. `@vfs` is
now required when a command first builds a VFS (`c354e37`), the lock reader by `drift` and the
YAML reader for a YAML lock alone (`c354e37`, `c060ba5`), and the reports, moved to a module of
their own (`12160eb`), when a command first has one to write (`93b3475`). `find-file` for a loose
file, `validate`, and anything that ends in a usage message build no VFS, and run in a third of
the time; the commands that do build one lose the YAML reader's 14 ms and nothing else, which is
the 1.07x to 1.09x. The package's own modules load in the same time they did.

Nothing else moved, because nothing else is this package's. A run of `lock` over the mod list,
timed by phase in a scratch copy with timers in it (once, not an A/B), is 124 ms of building the
VFS, 50 of the manifest, 9 of shaping the report and 33 of writing it as JSON (408 as YAML): the
first two are `@vfs` and the last is `@jess/data/value`. The shaping in the middle is this
package's, and is table allocation: `duplicates`, the heaviest, shapes for about 27 ms of its
230. Building its maps by hand instead of from pairs took that to about 21 in the same scratch
copy, which is under what the A/B can show, and was not kept.

### What the dialect cost here

The loops the dialect expresses exactly were converted (`5e57ff3`, `9e696ac`, `2a67123`: seven
comprehensions, an `any`, a slice, and fourteen record patterns, thirteen of them in the report
shapes), measured, and put back (`5864b68`). None of them is in a path that runs more than once per process, so
none could be faster warm, and each module that gained one loads slower. `cold.luau own`,
thirty-one runs of each, in ms; the middle rows are scratch builds with one module's conversions
taken out again:

| Build | require @vfstool | first validate | all four |
|---|---:|---:|---:|
| plain (`93b3475`) | 10.13 | 2.69 | 15.13 |
| converted (`2a67123`) | 11.20 | 3.09 | 16.57 |
| converted, `init.luau` plain (no slice) | 10.90 | 3.07 | 16.36 |
| converted, `commands.luau` plain (no `any`, no comprehensions) | 10.53 | 3.10 | 16.06 |
| converted, `reports.luau` without its two comprehensions | 11.29 | 3.00 | 16.60 |
| converted, `reports.luau` plain (no comprehensions, no patterns) | 11.15 | 2.71 | 16.19 |

That is 0.3 ms for `init.luau`'s one slice, 0.7 ms for `commands.luau`'s `any` and five
comprehensions, and 0.4 ms for `reports.luau`, 0.3 of it the record patterns, which are not free
at load: 1.1 ms on
every process and 0.4 more on one that writes a report. From outside, over forty-one rounds, the
six commands were each slower converted, by 0.2 to 2.6 ms (0.2 to 2.2 percent): under what this
protocol calls a change, and in the direction the table above predicts. A tool that runs once
and exits pays for a construct's lowering every time and never earns it back, so the payload
stays plain Luau; `git revert 5864b68` brings all three conversions back if the syntax is wanted
at that price. The benches and the suites use the dialect freely: nothing ships them.
