// SPDX-License-Identifier: MIT OR Apache-2.0
//! Luau boundary benchmarks: the cost of the `vfstool` calls a script makes most.
//!
//! The scripts are frozen at the pre-migration commit so the mlua and l3i numbers compare like
//! for like: the module reachable as the global `vfstool`, fixtures built by the same Luau
//! constructor call, `N` calls per script invocation where a call is cheap, one call where it
//! materialises a collection. Fixtures: a 1000-file directory (`vfs`) with one 64 KiB file, and a
//! 5000-file directory (`vfsLarge`). The l3i binding adds `read_into_64k` (the copy-free read),
//! `keys_5000_to_table` and `entries_1000_to_table` (the old materialised shapes, which the
//! frozen `keys_5000`/`entries_1000` scripts now measure as views), and `tree_1000_to_table`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use l3i::{
    Runtime,
    extension::{RuntimePlan, RuntimePolicy},
};
use vfstool_lib::lua::{MODULE, MODULE_NAME, VfsExtension};

const CALLS: u64 = 1000;

/// RAII temp directory under the system temp dir.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
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

/// `n` files spread across an OpenMW-like subdirectory layout, plus one 64 KiB payload.
fn make_fixture(name: &str, n: usize) -> TempDir {
    let dir = TempDir::new(name);
    let subdirs = ["textures", "meshes", "icons", "sound", "music", "scripts"];
    for i in 0..n {
        let subdir = subdirs[i % subdirs.len()];
        dir.write(&format!("{subdir}/file_{i:05}.dat"), b"x");
    }
    dir.write("data/big.bin", &vec![0xA5u8; 64 * 1024]);
    dir
}

/// Builds both fixtures through the module's own constructor; `N` is the per-script call count.
const SETUP: &str = r"
    N = CALLS
    vfs = vfstool.VFS.fromDirectories({ fixtureSmall })
    vfsLarge = vfstool.VFS.fromDirectories({ fixtureLarge })
    bigFile = vfs:getFile('data/big.bin')
";

/// `(name, calls per invocation, script returning the function to time)`.
const SCRIPTS: &[(&str, u64, &str)] = &[
    (
        "get_file_hit",
        CALLS,
        "return function() for _ = 1, N do vfs:getFile('textures/file_00006.dat') end end",
    ),
    (
        "get_file_hit_unnormalized",
        CALLS,
        "return function() for _ = 1, N do vfs:getFile('Textures\\\\File_00006.DAT') end end",
    ),
    (
        "get_file_miss",
        CALLS,
        "return function() for _ = 1, N do vfs:getFile('textures/missing.dat') end end",
    ),
    (
        "contains_hit",
        CALLS,
        "return function() for _ = 1, N do vfs:contains('meshes/file_00007.dat') end end",
    ),
    (
        "contains_miss",
        CALLS,
        "return function() for _ = 1, N do vfs:contains('meshes/missing.dat') end end",
    ),
    (
        "len_5000",
        CALLS,
        "return function() for _ = 1, N do vfsLarge:len() end end",
    ),
    (
        "keys_5000",
        1,
        "return function() return #vfsLarge:keys() end",
    ),
    (
        "entries_1000",
        1,
        "return function() return #vfs:entries() end",
    ),
    (
        "paths_matching_broad",
        1,
        "return function() return #vfs:pathsMatching('file_') end",
    ),
    (
        "paths_matching_narrow",
        1,
        "return function() return #vfs:pathsMatching('file_00999') end",
    ),
    (
        "paths_with",
        1,
        "return function() return #vfs:pathsWith('textures') end",
    ),
    (
        "providers_for_hit",
        CALLS,
        "return function() for _ = 1, N do vfs:providersFor('icons/file_00002.dat') end end",
    ),
    (
        "explain_hit",
        CALLS,
        "return function() for _ = 1, N do vfs:explain('icons/file_00002.dat') end end",
    ),
    ("tree_1000", 1, "return function() return vfs:tree() end"),
    (
        "read_all_64k",
        1,
        "return function() return #bigFile:readAll() end",
    ),
    (
        "file_path",
        CALLS,
        "return function() for _ = 1, N do bigFile:path() end end",
    ),
];

fn bench_boundary(c: &mut Criterion) {
    let small = make_fixture("vfstool_luau_small", 1000);
    let large = make_fixture("vfstool_luau_large", 5000);
    let plan = RuntimePlan::builder()
        .policy(RuntimePolicy::new().compat_global(MODULE, MODULE_NAME))
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()
        .expect("finalize the plan");
    let runtime = Runtime::from_plan(&plan).expect("a runtime from the plan");
    runtime
        .exec(&format!(
            "fixtureSmall = {:?} fixtureLarge = {:?}",
            small.path().to_string_lossy(),
            large.path().to_string_lossy()
        ))
        .expect("set fixture globals");
    runtime
        .exec(&SETUP.replace("CALLS", &CALLS.to_string()))
        .expect("build fixtures");
    let mut group = c.benchmark_group("luau_boundary");
    group.sample_size(30);
    for (name, calls, script) in SCRIPTS {
        group.throughput(Throughput::Elements(*calls));
        let function = runtime.load_function(script).expect("compile script");
        group.bench_function(*name, |b| {
            b.iter(|| {
                function
                    .invoke::<(), ()>(&runtime.stack(), ())
                    .expect("run script");
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_boundary);
criterion_main!(benches);
