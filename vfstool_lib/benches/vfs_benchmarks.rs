use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::{
    fs,
    path::{Path, PathBuf},
};
use vfstool_lib::{normalize_path, normalize_path_in_place, vfs::VFS};

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
        // allocating version — returns a new PathBuf
        g.bench_function(format!("allocating/{name}"), |b| {
            b.iter_batched(
                || PathBuf::from(input),
                |p| normalize_path(black_box(p.as_path())),
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

    // tree_filtered: the two-pass case (full tree then prune)
    g.bench_function("tree_filtered_extension_match", |b| {
        b.iter(|| {
            vfs.tree_filtered(true, |file| {
                file.path().extension().is_some_and(|e| e == "dat")
            })
        })
    });

    // tree_filtered: degenerate case — nothing matches, still builds full tree first
    g.bench_function("tree_filtered_none_match", |b| {
        b.iter(|| vfs.tree_filtered(true, |_| black_box(false)))
    });

    // tree_filtered: degenerate case — everything matches (equivalent to tree())
    g.bench_function("tree_filtered_all_match", |b| {
        b.iter(|| vfs.tree_filtered(true, |_| black_box(true)))
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
);

criterion_main!(benches);
