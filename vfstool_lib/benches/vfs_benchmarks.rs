use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::{
    fs,
    path::{Path, PathBuf},
};
use vfstool_lib::{ConflictIndex, VFS, normalize_path, normalize_path_in_place};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// RAII temp directory that cleans up on drop.
/// Uses the system temp dir so it doesn't pollute the project tree.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, data: &[u8]) {
        let target = self.0.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&target, data).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Create a fixture directory with `n` files spread across a realistic
/// OpenMW-like subdirectory layout.
fn make_fixture(name: &str, n: usize) -> TempDir {
    let dir = TempDir::new(name);
    let subdirs = ["textures", "meshes", "icons", "sound", "music", "scripts"];
    for i in 0..n {
        let subdir = subdirs[i % subdirs.len()];
        dir.write(&format!("{subdir}/file_{i:05}.dat"), b"x");
    }
    dir
}

// ---------------------------------------------------------------------------
// normalize_path
// ---------------------------------------------------------------------------

fn bench_normalize(c: &mut Criterion) {
    let mut g = c.benchmark_group("normalize_path");

    g.bench_function("already_normalized", |b| {
        b.iter(|| normalize_path(black_box("textures/landscape/foo.dds")))
    });

    g.bench_function("backslash_only", |b| {
        b.iter(|| normalize_path(black_box("textures\\landscape\\foo.dds")))
    });

    g.bench_function("uppercase_only", |b| {
        b.iter(|| normalize_path(black_box("Meshes/Actors/XBase_Anim.NIF")))
    });

    g.bench_function("combined_case_and_backslash", |b| {
        b.iter(|| normalize_path(black_box("Meshes\\Actors\\XBase_Anim.NIF")))
    });

    g.bench_function("long_path_combined", |b| {
        b.iter(|| {
            normalize_path(black_box(
                "Data Files\\Textures\\Landscape\\TX_BC_rock_04.DDS",
            ))
        })
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// normalize_path_in_place
// ---------------------------------------------------------------------------

fn bench_normalize_in_place(c: &mut Criterion) {
    let mut g = c.benchmark_group("normalize_path_in_place");

    // Fast path: the function is a pure no-op on already-normalized input.
    // We can hold a single PathBuf across all iterations — it is never
    // modified, so no per-iteration setup is needed and there is zero
    // allocation overhead inflating the measurement.
    g.bench_function("already_normalized", |b| {
        let mut p = PathBuf::from("textures/landscape/foo.dds");
        b.iter(|| normalize_path_in_place(black_box(&mut p)))
    });

    // Slow paths: the PathBuf is modified in place, so each iteration must
    // start from a fresh copy.  iter_batched with SmallInput amortises the
    // setup-closure overhead across a batch, keeping it out of the hot loop.
    g.bench_function("combined_case_and_backslash", |b| {
        b.iter_batched(
            || PathBuf::from("Meshes\\Actors\\XBase_Anim.NIF"),
            |mut p| {
                normalize_path_in_place(black_box(&mut p));
                p
            },
            BatchSize::SmallInput,
        )
    });

    g.bench_function("long_path_combined", |b| {
        b.iter_batched(
            || PathBuf::from("Data Files\\Textures\\Landscape\\TX_BC_rock_04.DDS"),
            |mut p| {
                normalize_path_in_place(black_box(&mut p));
                p
            },
            BatchSize::SmallInput,
        )
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Direct comparison: same owned PathBuf input, both functions
//
// This isolates just the normalization work by giving both functions
// identical starting state — an already-heap-allocated PathBuf — and
// measuring only what happens after that point.
//
// normalize_path(&p)          → scans, may allocate a new PathBuf
// normalize_path_in_place(&mut p) → scans, may transform in place
//
// Both use iter_batched so setup (PathBuf::from) is excluded.
// ---------------------------------------------------------------------------

fn bench_normalize_comparison(c: &mut Criterion) {
    // Inputs chosen to cover the three normalization scenarios.
    let cases: &[(&str, &str)] = &[
        ("already_normalized",         "textures/landscape/foo.dds"),
        ("combined_case_and_backslash", "Meshes\\Actors\\XBase_Anim.NIF"),
        ("long_path_combined",          "Data Files\\Textures\\Landscape\\TX_BC_rock_04.DDS"),
    ];

    let mut g = c.benchmark_group("normalize_comparison");

    for &(name, input) in cases {
        // allocating version — returns a new PathBuf (into_owned avoids borrow-from-local)
        g.bench_function(format!("allocating/{name}"), |b| {
            b.iter_batched(
                || PathBuf::from(input),
                |p| normalize_path(black_box(p.as_path())).into_owned(),
                BatchSize::SmallInput,
            )
        });

        // in-place version — modifies the PathBuf, returns it to prevent DCE
        g.bench_function(format!("in_place/{name}"), |b| {
            b.iter_batched(
                || PathBuf::from(input),
                |mut p| {
                    normalize_path_in_place(black_box(&mut p));
                    p
                },
                BatchSize::SmallInput,
            )
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// VFS construction
// ---------------------------------------------------------------------------

fn bench_construction(c: &mut Criterion) {
    let mut g = c.benchmark_group("vfs_construction");
    g.sample_size(20); // construction is I/O-bound; fewer samples keep CI fast

    for &n in &[100usize, 500, 2000] {
        let fixture = make_fixture(&format!("vfsbench_construct_{n}"), n);

        g.bench_with_input(BenchmarkId::from_parameter(n), &fixture, |b, dir| {
            b.iter(|| VFS::from_directories(vec![black_box(dir.path())], None))
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// VFS lookup
// ---------------------------------------------------------------------------

fn bench_lookup(c: &mut Criterion) {
    let fixture = make_fixture("vfsbench_lookup", 1000);
    let vfs = VFS::from_directories(vec![fixture.path()], None);

    let mut g = c.benchmark_group("vfs_lookup");

    // Key exists, already normalized — exercises the fast path
    g.bench_function("hit_normalized", |b| {
        b.iter(|| vfs.get_file(black_box("textures/file_00000.dat")))
    });

    // Key exists, needs case folding — exercises normalize_path before lookup
    g.bench_function("hit_uppercase", |b| {
        b.iter(|| vfs.get_file(black_box("Textures/File_00000.dat")))
    });

    // Key exists, needs backslash conversion
    g.bench_function("hit_backslash", |b| {
        b.iter(|| vfs.get_file(black_box("textures\\file_00000.dat")))
    });

    // Key exists, needs both — worst-case normalization before hit
    g.bench_function("hit_combined", |b| {
        b.iter(|| vfs.get_file(black_box("Textures\\File_00000.dat")))
    });

    // Key does not exist — exercises full normalization + HashMap miss
    g.bench_function("miss", |b| {
        b.iter(|| vfs.get_file(black_box("textures/no_such_file.dds")))
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// VFS search (paths_matching / paths_with)
// ---------------------------------------------------------------------------

fn bench_search(c: &mut Criterion) {
    let fixture = make_fixture("vfsbench_search", 1000);
    let vfs = VFS::from_directories(vec![fixture.path()], None);

    let mut g = c.benchmark_group("vfs_search");

    // paths_matching: broad — hits ~1/6 of all entries
    g.bench_function("paths_matching_broad", |b| {
        b.iter(|| vfs.paths_matching(black_box("textures")).count())
    });

    // paths_matching: narrow — single unique entry
    g.bench_function("paths_matching_narrow", |b| {
        b.iter(|| vfs.paths_matching(black_box("file_00042")).count())
    });

    // paths_matching: miss — scans everything, returns nothing
    g.bench_function("paths_matching_miss", |b| {
        b.iter(|| vfs.paths_matching(black_box("sprites")).count())
    });

    // paths_with: broad prefix
    g.bench_function("paths_with_broad", |b| {
        b.iter(|| vfs.paths_with(black_box("textures")).count())
    });

    // paths_with: miss
    g.bench_function("paths_with_miss", |b| {
        b.iter(|| vfs.paths_with(black_box("sprites")).count())
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tree construction (the slow path we're planning to fix)
// ---------------------------------------------------------------------------

fn bench_tree(c: &mut Criterion) {
    let fixture = make_fixture("vfsbench_tree", 500);
    let vfs = VFS::from_directories(vec![fixture.path()], None);

    let mut g = c.benchmark_group("vfs_tree");
    g.sample_size(20);

    g.bench_function("tree_full_relative", |b| {
        b.iter(|| vfs.tree(black_box(true)))
    });

    g.bench_function("tree_full_absolute", |b| {
        b.iter(|| vfs.tree(black_box(false)))
    });

    // tree_filtered: filter by extension — exercises the key+file predicate
    g.bench_function("tree_filtered_extension_match", |b| {
        b.iter(|| {
            vfs.tree_filtered(true, |_key, file| {
                file.path().extension().is_some_and(|e| e == "dat")
            })
        })
    });

    // tree_filtered: degenerate case — nothing matches
    g.bench_function("tree_filtered_none_match", |b| {
        b.iter(|| vfs.tree_filtered(true, |_, _| black_box(false)))
    });

    // tree_filtered: degenerate case — everything matches (equivalent to tree())
    g.bench_function("tree_filtered_all_match", |b| {
        b.iter(|| vfs.tree_filtered(true, |_, _| black_box(true)))
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// diff_directory
// ---------------------------------------------------------------------------

fn bench_diff(c: &mut Criterion) {
    // VFS built from a 1000-file base directory
    let base = make_fixture("vfsbench_diff_base", 1000);
    let vfs = VFS::from_directories(vec![base.path()], None);

    // Mod directory: 100% overlap with the VFS (worst case for conflict detection)
    let all_conflict = make_fixture("vfsbench_diff_allconflict", 1000);

    // Mod directory: 0% overlap (all additions — no HashMap hits)
    let all_new = TempDir::new("vfsbench_diff_allnew");
    for i in 0..500 {
        all_new.write(&format!("scripts/new_{i:05}.lua"), b"x");
    }

    // Mod directory: 50/50 mix — half conflict, half addition
    let mixed = TempDir::new("vfsbench_diff_mixed");
    for i in 0..250 {
        // These keys collide with base fixture (same subdir/filename pattern)
        mixed.write(&format!("textures/file_{i:05}.dat"), b"x");
    }
    for i in 0..250 {
        mixed.write(&format!("scripts/new_{i:05}.lua"), b"x");
    }

    let mut g = c.benchmark_group("vfs_diff");
    g.sample_size(20);

    g.bench_function("all_conflict_1000", |b| {
        b.iter(|| vfs.diff_directory(black_box(all_conflict.path())))
    });

    g.bench_function("all_addition_500", |b| {
        b.iter(|| vfs.diff_directory(black_box(all_new.path())))
    });

    g.bench_function("mixed_500", |b| {
        b.iter(|| vfs.diff_directory(black_box(mixed.path())))
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// ConflictIndex
// ---------------------------------------------------------------------------

fn bench_conflict_index(c: &mut Criterion) {
    // Simulate a realistic load order:
    //   base: 1000 files, spread across subdirs
    //   mod_a: 200 files, 100 conflicting with base
    //   mod_b: 200 files, 50 conflicting with base, 25 conflicting with mod_a

    let base = make_fixture("vfsbench_ci_base", 1000);

    let mod_a = TempDir::new("vfsbench_ci_mod_a");
    // 100 files that collide with base (same names)
    for i in 0..100 {
        mod_a.write(&format!("textures/file_{i:05}.dat"), b"x");
    }
    // 100 files unique to mod_a
    for i in 0..100 {
        mod_a.write(&format!("meshes/mod_a_{i:05}.nif"), b"x");
    }

    let mod_b = TempDir::new("vfsbench_ci_mod_b");
    // 50 files colliding with base
    for i in 0..50 {
        mod_b.write(&format!("textures/file_{i:05}.dat"), b"x");
    }
    // 25 files colliding with mod_a
    for i in 0..25 {
        mod_b.write(&format!("meshes/mod_a_{i:05}.nif"), b"x");
    }
    // 125 unique files
    for i in 0..125 {
        mod_b.write(&format!("icons/mod_b_{i:05}.dds"), b"x");
    }

    let mut g = c.benchmark_group("conflict_index");
    g.sample_size(20);

    // Two-directory case: base + one mod
    g.bench_function("two_dirs_1000_plus_200", |b| {
        b.iter(|| {
            ConflictIndex::from_directories(black_box(vec![base.path(), mod_a.path()]))
        })
    });

    // Three-directory case: full realistic load order
    g.bench_function("three_dirs_1000_200_200", |b| {
        b.iter(|| {
            ConflictIndex::from_directories(black_box(vec![
                base.path(),
                mod_a.path(),
                mod_b.path(),
            ]))
        })
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Serialization (gated on the serialize feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "serialize")]
fn bench_serialize(c: &mut Criterion) {
    use vfstool_lib::SerializeType;

    let fixture = make_fixture("vfsbench_serialize", 500);
    let vfs = VFS::from_directories(vec![fixture.path()], None);
    let tree = vfs.tree(true);

    let mut g = c.benchmark_group("vfs_serialize");
    g.sample_size(20);

    g.bench_function("json", |b| {
        b.iter(|| VFS::serialize_from_tree(black_box(&tree), SerializeType::Json))
    });

    g.bench_function("yaml", |b| {
        b.iter(|| VFS::serialize_from_tree(black_box(&tree), SerializeType::Yaml))
    });

    g.bench_function("toml", |b| {
        b.iter(|| VFS::serialize_from_tree(black_box(&tree), SerializeType::Toml))
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// criterion_group wiring
// ---------------------------------------------------------------------------

#[cfg(feature = "serialize")]
criterion_group!(
    benches,
    bench_normalize,
    bench_normalize_in_place,
    bench_normalize_comparison,
    bench_construction,
    bench_lookup,
    bench_search,
    bench_tree,
    bench_diff,
    bench_conflict_index,
    bench_serialize,
);

#[cfg(not(feature = "serialize"))]
criterion_group!(
    benches,
    bench_normalize,
    bench_normalize_in_place,
    bench_normalize_comparison,
    bench_construction,
    bench_lookup,
    bench_search,
    bench_tree,
    bench_diff,
    bench_conflict_index,
);

criterion_main!(benches);
