# vfstool benchmarks

Four benches. A command-line tool has three costs, and the fourth is for a lookup none of those
three is large enough to show.

`bench/bench.jsl` times the commands warm, through the package's one export, `run`: each over a
synthetic mod list written under a scratch folder (240 data folders of 25 files, 200 plugins and
6 groundcover plugins beside them, and a 2,500-entry BSA: 3,816 keys), its report written to a
file. One line per workload: its name, a tab, the seconds one run of the command takes with the
configuration, the VFS and the report included. Each is warmed up, sized to a batch of about a
tenth of a second, and reported as the median of seven batches with a full collection between
them. Every run's exit code is checked, and its report is held to a count the mod list's
generator works out without the port. The reports stay in `<scratch>/vfstool-bench/reports`: two
builds run over the same scratch folder must leave the same bytes there.

`bench/cold.jsl` times what one process pays once, from inside it: `@vfstool` required, then the
first `validate`, `lock` and `drift` over three folders, each of which compiles what it is the
first to need. With `own`, every module of another package is loaded first, so the lines time
this package's own modules alone. Each line is one sample, so it is run many times and the
medians are compared.

`bench/process.jsl` times whole processes from outside: `jess package run` of each package
folder it is given, turn and turn about, for six commands over three folders, and for two more
over `bench.jsl`'s mod list when that has been left in the same scratch folder.

`bench/crowded.jsl` times loose lookups in one data folder of 20,000 names, where what a lookup
costs is reading the folder: `find-file` for one name, and `validate` for one and for fifty. Each
line is the median of nine runs after one that is not kept. `find-file` prints the path it finds,
so the lines come after the paths.

```sh
jess run --package jpk/vfstool --package-path jpk \
  --capability filesystem.read --capability filesystem.write \
  jpk/vfstool/bench/bench.jsl <scratch folder> [filter]

jess run --package jpk/vfstool --package-path jpk \
  --capability filesystem.read --capability filesystem.write \
  jpk/vfstool/bench/cold.jsl <scratch folder> [own]

jess run --capability filesystem.read --capability filesystem.write --capability process.spawn \
  jpk/vfstool/bench/process.jsl <jess> <package path> <scratch folder> <rounds> <package folder>...

jess run --package jpk/vfstool --package-path jpk \
  --capability filesystem.read --capability filesystem.write \
  jpk/vfstool/bench/crowded.jsl <scratch folder> [names]
```

## The move onto l3i 2.0, base against head

- Machine: Intel Core i7-10870H @ 2.20 GHz, 16 threads, shared with other work while this ran.
- Date: 2026-10-12, the runs ending after midnight.
- Binary: the verified release build of Jess 0.1.0 on l3i 2.0.2 (Luau 0.742), the same one for
  both sides.
- Base: `0dda493`, the package before the move, with the reports suite and the benches. Head:
  the source of `5864b68` in the warm, whole-process and cold tables. `456c6a7` after it changes
  `find-file`'s loose lookup and nothing else, and is the head of the table of one folder of
  20,000 names; the rows of the others it could move were run again with it and are under that
  table.
- Protocol: every A/B alternates base and head, back to back, under one lock. `bench.jsl`: three
  runs of each, each cell the median of its three. `cold.jsl`: thirty-one runs of each, in each
  mode. `process.jsl`: twenty-one rounds. `crowded.jsl`: five runs of each, each cell the median
  of its five. A difference under 5 percent is called no change.

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
| first validate, other packages loaded | 0.29 | 2.77 | pays for `reports.jsl` |
| first lock, other packages loaded | 0.82 | 0.86 | no change |
| first drift, other packages loaded | 0.76 | 1.50 | pays for `lock.jsl` |
| all four, other packages loaded | 14.97 | 15.52 | no change |

### One folder of 20,000 names

| Workload | Base, ms | `5864b68`, ms | Head, ms | Base / head |
|---|---:|---:|---:|---:|
| find-file -s, the last of 20,000 names | 10.03 | 13.58 | 9.82 | no change |
| validate, one name of 20,000 | 9.90 | 13.09 | 13.27 | 0.75x, slower |
| validate, 50 names of 20,000 | 422.30 | 13.41 | 13.15 | 32.1x |

In the same session, three runs of `bench.jsl` a side and twenty-one rounds of `process.jsl`,
`5864b68` against head: warm `validate` 7.64 and 7.57 ms, warm `find-file -s, a file nothing has`
120.26 and 120.90, and each of the eight processes within 1.4 ms of itself. No change.

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
first read.

A listing by key is a table for every name in the folder, which one lookup there does not earn
back. `find-file` looks for one name, and `a9e8c68` gave it the listing too: over folders
of a few names that is the same speed, and in a folder of 20,000 it was 13.6 ms where reading the
folder through had been 10.0. `find-file` reads the folder through again (`456c6a7`). `validate`
keeps the listing whatever it looks for, so a configuration that names one loose file in a folder
that large pays the 3 ms, and one that names fifty there is 32 times ahead.

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
none could be faster warm, and each module that gained one loads slower. `cold.jsl own`,
thirty-one runs of each, in ms; the middle rows are scratch builds with one module's conversions
taken out again:

| Build | require @vfstool | first validate | all four |
|---|---:|---:|---:|
| plain (`93b3475`) | 10.13 | 2.69 | 15.13 |
| converted (`2a67123`) | 11.20 | 3.09 | 16.57 |
| converted, `init.jsl` plain (no slice) | 10.90 | 3.07 | 16.36 |
| converted, `commands.jsl` plain (no `any`, no comprehensions) | 10.53 | 3.10 | 16.06 |
| converted, `reports.jsl` without its two comprehensions | 11.29 | 3.00 | 16.60 |
| converted, `reports.jsl` plain (no comprehensions, no patterns) | 11.15 | 2.71 | 16.19 |

That is 0.3 ms for `init.jsl`'s one slice, 0.7 ms for `commands.jsl`'s `any` and five
comprehensions, and 0.4 ms for `reports.jsl`, 0.3 of it the record patterns, which are not free
at load: 1.1 ms on
every process and 0.4 more on one that writes a report. From outside, over forty-one rounds, the
six commands were each slower converted, by 0.2 to 2.6 ms (0.2 to 2.2 percent): under what this
protocol calls a change, and in the direction the table above predicts. A tool that runs once
and exits pays for a construct's lowering every time and never earns it back, so the payload
stays plain Luau; `git revert 5864b68` brings all three conversions back if the syntax is wanted
at that price. The benches and the suites use the dialect freely: nothing ships them.

## Which modules are native, and why

Jess compiles to machine code only what is marked (`--!native` on a module, `@native` on one
function), and every load pays for what is marked. A module carries `--!native` when a row below
spends its time there and the module is small (12 KB or less); a larger module carries `@native`
on the functions the row spends its time in, and nothing else.

Measured 2026-10-15 with `jess 0.1.0`, the verified release build of jess `07c38cb` on l3i
2.0.3, on the i7-10870H, the one-minute load 1.07 and 1.20 at the start of the two
sessions. In each session the bench ran alternately on this package and on a copy of it with
every module marked, three fresh runs of `bench/bench.jsl` each, every run's row the median of
its batches; a column is the median of the three, and a ratio is against the all-marked runs of
the same session. Which functions a row spends its time in comes from `jess run --profile` of
that row alone, unmarked. The load cost is a fresh `jess run --package <this package>
--package-path jpk` of a script that only requires `@vfstool`, the two sides alternating, the
median of five each after one warm-up.

| Requiring `@vfstool` | All marked | This state |
| --- | ---: | ---: |
| Unmarked | 0.063 s | 0.054 s |
| Final | 0.061 s | 0.055 s |

| Row | All marked (ms) | Unmarked / all marked | Final (ms) | Final / all marked |
| --- | ---: | ---: | ---: | ---: |
| validate | 7.094 | **1.158×** | 7.197 | 1.014× |
| validate --full | 105.2 | 1.026× | 104.8 | 0.997× |
| find-file -s, a file nothing has | 113.4 | 1.023× | 112.7 | 0.993× |
| explain | 106.5 | 1.030× | 102.5 | 0.963× |
| archives | 104.8 | 1.013× | 103.2 | 0.984× |
| find -r | 106.6 | 1.019× | 105.6 | 0.990× |
| remaining -r | 138.9 | 1.015× | 135.3 | 0.974× |
| duplicates | 233.4 | 1.024× | 224.2 | 0.961× |
| archive-list | 129.2 | **1.080×** | 126.0 | 0.975× |
| contributions | 116.1 | **1.065×** | 112.9 | 0.972× |
| conflicts | 166.2 | **1.070×** | 161.9 | 0.974× |
| conflicts -r | 138.2 | **1.072×** | 131.4 | 0.951× |
| shadowed -l | 117.9 | **1.063×** | 115.5 | 0.980× |
| diff | 119.5 | **1.056×** | 116.5 | 0.975× |
| collapse --dry-run, json | 201.4 | **1.092×** | 199.3 | 0.990× |
| collapse --dry-run, yaml | 496.8 | **1.074×** | 488.7 | 0.984× |
| lock, json | 201.2 | **1.075×** | 192.1 | 0.955× |
| lock, yaml | 538.0 | **1.089×** | 531.4 | 0.988× |
| lock, toml | 255.7 | **1.084×** | 249.8 | 0.977× |
| drift from a json lock, none | 195.4 | **1.072×** | 189.5 | 0.970× |
| drift from a yaml lock, none | 398.7 | **1.079×** | 386.7 | 0.970× |
| drift from a toml lock, none | 182.3 | **1.073×** | 175.7 | 0.964× |
| drift from a json lock, forty folders added | 205.8 | **1.079×** | 200.2 | 0.973× |

What carries the directive (every module and function here is one a row spends its time in,
unmarked):

- Whole: `reports`.
- `commands`, by function: `listed`, `findLoose`.
- Unmarked: `cli`, `init`, `lock`, `main`.

Unmarked, 16 of the 23 rows ran more than 5% slower: the reports and the listings walk every
file of a VFS. With these marked, none is, and requiring `@vfstool` costs 6 ms less than all
marked; the VFS's own hot paths are `@vfs`'s to mark, and are (see its BENCHMARKS.md).
