// SPDX-License-Identifier: MIT OR Apache-2.0
//! Luau boundary benchmarks: the cost of the `vfstool` calls a script makes most.
//!
//! The scripts are frozen at the pre-migration commit so the mlua and l3i numbers compare like
//! for like: the module reachable as the global `vfstool`, fixtures built by the same Luau
//! constructor call, `N` calls per script invocation where a call is cheap, one call where it
//! materialises a collection. Fixtures: a 1000-file directory (`vfs`) with one 64 KiB file, and a
//! 5000-file directory (`vfsLarge`). The l3i binding adds `read_into_64k` (the copy-free read),
//! `keys_5000_to_table` and `entries_1000_to_table` (the old materialised shapes, which the
//! frozen `keys_5000`/`entries_1000` scripts now measure as views), `tree_1000_to_table`, and
//! the `serialize_*` scripts (a 5000-key list as a table and as a view, and the nested tree),
//! which need the `serialize` feature as well.
//!
//! `luau_io` measures the file I/O calls against their plain Rust equivalents, so the report
//! shows the boundary's cost: `readAt` of 4 KiB and 1 MiB from an 8 MiB loose file against
//! `File::read_at`, `readRange` of 64 KiB against a memcpy, a reader walking the 8 MiB file in
//! 16 KiB chunks against a `File::read` loop, `vfs:writeFile` of 1 MiB against `fs::write`, and
//! `host.readFile` of 1 MiB against `fs::read`. `luau_require` measures 100 sandbox instances each
//! requiring the same 200-line module, with the template cache (one compile, 100 clones) and
//! without it (100 compiles), as time per require.

use std::{
    fmt::Write as _,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    rc::Rc,
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use l3i::{
    Runtime,
    extension::{RuntimePlan, RuntimePolicy},
    sandbox::{Instance, InstanceSpec, SandboxOptions},
};
use vfstool_lib::VFS;
use vfstool_lib::lua::{
    MODULE, MODULE_NAME, TemplateCache, Vfs, VfsExtension, VfsRequireNavigator,
};

const CALLS: u64 = 1000;
const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

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
    (
        "read_into_64k",
        1,
        "local buf = buffer.create(65536) return function() return bigFile:readInto(buf, 0) end",
    ),
    (
        "keys_5000_to_table",
        1,
        "return function() return #vfsLarge:keys():toTable() end",
    ),
    (
        "entries_1000_to_table",
        1,
        "return function() return #vfs:entries():toTable() end",
    ),
    (
        "tree_1000_to_table",
        1,
        "return function() return vfs:tree():toTable() end",
    ),
    (
        "serialize_keys_5000_table",
        1,
        "local keys = vfsLarge:keys():toTable() return function() return #vfstool.serialize(keys, 'json') end",
    ),
    (
        "serialize_keys_5000_view",
        1,
        "local keys = vfsLarge:keys() return function() return #vfstool.serialize(keys, 'json') end",
    ),
    (
        "serialize_tree_1000_table",
        1,
        "local function plain(node) \
            local out = { files = {}, subdirs = {} } \
            for i, row in node.files do out.files[i] = { path = row.path, isLoose = row.isLoose } end \
            for name, sub in node.subdirs do out.subdirs[name] = plain(sub) end \
            return out \
        end \
        local tree = {} \
        for root, node in vfs:tree():toTable() do tree[root] = plain(node) end \
        return function() return #vfstool.serialize(tree, 'json') end",
    ),
];

/// A repeating byte pattern of `len` bytes.
fn pattern(len: usize) -> Vec<u8> {
    (0..=255u8).cycle().take(len).collect()
}

/// An 8 MiB and a 1 MiB file, and the write root the write scripts use.
fn make_io_fixture() -> TempDir {
    let dir = TempDir::new("vfstool_luau_io");
    dir.write("data/eight.bin", &pattern(8 * MIB));
    dir.write("data/one.bin", &pattern(MIB));
    fs::create_dir_all(dir.path().join("out")).unwrap();
    dir
}

const IO_SETUP: &str = r"
    N = CALLS
    ioVfs = vfstool.VFS.fromDirectories({ fixtureIo .. '/data' }, { writeRoot = fixtureIo .. '/out' })
    eight = ioVfs:getFile('eight.bin')
    one = ioVfs:getFile('one.bin')
    oneBuf = one:readAllBuffer()
";

/// `(name, calls per invocation, script returning the function to time)`.
const IO_SCRIPTS: &[(&str, u64, &str)] = &[
    (
        "read_at_4k",
        CALLS,
        "local buf = buffer.create(4096) return function() for i = 1, N do eight:readAt(buf, (i % 2048) * 4096) end end",
    ),
    (
        "read_at_1m",
        1,
        "local buf = buffer.create(1048576) return function() return eight:readAt(buf, 1048576) end",
    ),
    (
        "read_range_64k",
        1,
        "return function() return eight:readRange(65536, 65536) end",
    ),
    (
        "reader_16k_over_8m",
        1,
        "local buf = buffer.create(16384) return function() local r = eight:open() local n = 0 while true do local got = r:readInto(buf) if got == 0 then break end n += got end r:close() return n end",
    ),
    (
        "write_file_1m",
        1,
        "return function() return ioVfs:writeFile('one.bin', oneBuf) end",
    ),
    (
        "host_read_file_1m",
        1,
        "return function() return vfstool.host.readFile(fixtureIo .. '/data/one.bin') end",
    ),
];

fn bench_io(c: &mut Criterion) {
    let fixture = make_io_fixture();
    let plan = RuntimePlan::builder()
        .policy(
            RuntimePolicy::new()
                .capability(vfstool_lib::lua::WRITE_CAPABILITY)
                .capability(vfstool_lib::lua::HOST_CAPABILITY)
                .compat_global(MODULE, MODULE_NAME),
        )
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()
        .expect("finalize the plan");
    let runtime = Runtime::from_plan(&plan).expect("a runtime from the plan");
    runtime
        .exec(&format!(
            "fixtureIo = {:?}",
            fixture.path().to_string_lossy()
        ))
        .expect("set fixture globals");
    runtime
        .exec(&IO_SETUP.replace("CALLS", &CALLS.to_string()))
        .expect("build fixtures");
    let mut group = c.benchmark_group("luau_io");
    group.sample_size(30);
    for (name, calls, script) in IO_SCRIPTS {
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
    // The plain Rust equivalents.
    let eight = fixture.path().join("data/eight.bin");
    let one = fixture.path().join("data/one.bin");
    let out = fixture.path().join("out/rust.bin");
    let file = fs::File::open(&eight).unwrap();
    let one_bytes = fs::read(&one).unwrap();
    let eight_bytes = fs::read(&eight).unwrap();
    group.throughput(Throughput::Elements(CALLS));
    group.bench_function("rust_read_at_4k", |b| {
        let mut buf = vec![0u8; 4 * KIB];
        b.iter(|| {
            for i in 0..CALLS {
                read_at(&file, &mut buf, (i % 2048) * 4096);
            }
        });
    });
    group.throughput(Throughput::Elements(1));
    group.bench_function("rust_read_at_1m", |b| {
        let mut buf = vec![0u8; MIB];
        b.iter(|| read_at(&file, &mut buf, MIB as u64));
    });
    group.bench_function("rust_memcpy_64k", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; 64 * KIB];
            buf.copy_from_slice(&eight_bytes[64 * KIB..128 * KIB]);
            buf
        });
    });
    group.bench_function("rust_read_seq_16k_over_8m", |b| {
        let mut buf = vec![0u8; 16 * KIB];
        b.iter(|| {
            let mut file = fs::File::open(&eight).unwrap();
            let mut total = 0;
            loop {
                let got = file.read(&mut buf).unwrap();
                if got == 0 {
                    break;
                }
                total += got;
            }
            total
        });
    });
    group.bench_function("rust_write_1m", |b| {
        b.iter(|| fs::write(&out, &one_bytes).unwrap());
    });
    group.bench_function("rust_read_1m", |b| {
        b.iter(|| fs::read(&one).unwrap());
    });
    group.finish();
}

/// `File::read_at` on Unix, `seek_read` on Windows, filling `buf`.
fn read_at(file: &fs::File, buf: &mut [u8], offset: u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buf, offset).unwrap();
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(buf, offset).unwrap();
    }
}

/// A 200-line module: a table of functions.
fn make_require_fixture() -> TempDir {
    let dir = TempDir::new("vfstool_luau_require");
    let mut source = String::from("local M = {}\n");
    for i in 1..=198 {
        writeln!(source, "M.f{i} = function(x) return x * {i} + {i} end").unwrap();
    }
    source.push_str("return M\n");
    dir.write("scripts/big.luau", source.as_bytes());
    dir
}

const INSTANCES: usize = 100;

/// `INSTANCES` sandbox instances each requiring the same module: with the template cache, one
/// compile and `INSTANCES` clones; without it (cleared before every require), `INSTANCES`
/// compiles. Luau's own result cache is cleared between instances, so each require reaches the
/// navigator, as a host giving each instance its own module state does.
fn bench_require(c: &mut Criterion) {
    let fixture = make_require_fixture();
    let plan = RuntimePlan::builder()
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()
        .expect("finalize the plan");
    let runtime = Rc::new(Runtime::from_plan(&plan).expect("a runtime from the plan"));
    let sandbox = Rc::new(
        runtime
            .sandbox(|_| {}, SandboxOptions::default())
            .expect("a sandbox"),
    );
    let vfs = Vfs::new(VFS::from_directories([fixture.path()], None));
    let cache = Rc::new(TemplateCache::new());
    runtime
        .install_require(VfsRequireNavigator::with_cache(
            vfs,
            sandbox.clone(),
            cache.clone(),
        ))
        .expect("install require");
    let require = runtime.global("require").expect("Luau's require");
    let loader = runtime
        .bind_function("dream.vfs.bench.loader", |name: &str| -> l3i::Result<()> {
            Err(l3i::Error::runtime(format!("no package '{name}'")))
        })
        .expect("a loader");
    let instances: Vec<Instance> = (0..INSTANCES)
        .map(|index| {
            let spec = InstanceSpec {
                name: &format!("instance{index}"),
                packages: &[],
                hidden_data: None,
                loader: &loader,
            };
            let instance = sandbox.new_instance(&runtime, &spec).expect("an instance");
            instance
                .env
                .set(&runtime.stack(), "require", &require)
                .expect("set require");
            instance
        })
        .collect();
    let script = sandbox
        .load_template(
            &runtime,
            "@scripts/entry.luau",
            "return require('./big').f7(1)",
        )
        .expect("the entry script");
    let mut group = c.benchmark_group("luau_require");
    group.sample_size(30);
    group.throughput(Throughput::Elements(INSTANCES as u64));
    for (name, cached) in [("require_cached", true), ("require_uncached", false)] {
        group.bench_function(name, |b| {
            b.iter(|| {
                for instance in &instances {
                    runtime.clear_require_cache().expect("clear Luau's cache");
                    if !cached {
                        cache.clear();
                    }
                    sandbox
                        .run(
                            &runtime,
                            &script,
                            instance,
                            runtime.initialization_context(),
                        )
                        .expect("require the module");
                }
            });
        });
    }
    assert!(
        cache.compiles() > INSTANCES,
        "the uncached run compiled per instance"
    );
    group.finish();
}

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

criterion_group!(benches, bench_boundary, bench_io, bench_require);
criterion_main!(benches);
