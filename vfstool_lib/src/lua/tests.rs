// SPDX-License-Identifier: MIT OR Apache-2.0
//! The extension's own gates: the plan finalizes with the expected tags, the declared types
//! check in Luau's frontend, strict scripts type check against the stubs, and the shapes the
//! docs promise hold at runtime.

use std::{fs, path::PathBuf, rc::Rc};

use l3i::{
    Runtime,
    extension::{RuntimePlan, RuntimePolicy, TagPolicy},
};

use super::{MODULE, MODULE_NAME, VfsExtension};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "vfstool_lua_unit_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }

    fn lua(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn plan() -> Rc<RuntimePlan> {
    RuntimePlan::builder()
        .policy(RuntimePolicy::new().compat_global(MODULE, MODULE_NAME))
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()
        .expect("the extension finalizes")
}

fn runtime() -> Runtime {
    Runtime::from_plan(&plan()).expect("a runtime from the plan")
}

#[test]
fn the_plan_tags_the_hot_types_only_and_requires_dream_path() {
    let plan = plan();
    assert_eq!(
        plan.installation_order(),
        ["dream.net", "dream.path", "dream.vfs"]
    );
    let tagged: Vec<&str> = plan
        .userdata()
        .iter()
        .filter(|u| u.owner == super::EXTENSION_ID && u.tag.is_some())
        .map(|u| u.key.as_str())
        .collect();
    assert_eq!(
        tagged,
        [
            "dream.vfs.Entries",
            "dream.vfs.Keys",
            "dream.vfs.Reader",
            "dream.vfs.VFS",
            "dream.vfs.VfsFile"
        ]
    );
    for cold in [
        "dream.vfs.VfsProvider",
        "dream.vfs.LayerIndex",
        "dream.vfs.ConflictIndex",
        "dream.vfs.VfsLock",
        "dream.vfs.Tree",
    ] {
        let resolved = plan.userdata_by_key(cold).unwrap();
        assert_eq!(resolved.policy, TagPolicy::Never, "{cold}");
        assert_eq!(resolved.tag, None, "{cold}");
    }
    // Without dream.path the plan does not finalize.
    let error = RuntimePlan::builder()
        .extension(VfsExtension)
        .finalize()
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("requires 'dream.path'"), "{error}");
}

#[cfg(feature = "luau-analysis")]
#[test]
fn the_declared_types_check_and_a_strict_script_type_checks() {
    let plan = plan();
    plan.check_definitions().expect("the declared types check");
    let definitions = plan.type_definitions();
    for expected in [
        "declare extern type dream_vfs_VFS with",
        "function getFile(self, path: string): dream_vfs_VfsFile?",
        "function readInto(self, buffer: buffer, offset: number?): number",
        "function readAt(self, buffer: buffer, fileOffset: number, length: number?, bufferOffset: number?): number",
        "function readRange(self, fileOffset: number, length: number): buffer",
        "function open(self): dream_vfs_Reader",
        "declare extern type dream_vfs_Reader with",
        #[cfg(feature = "lua-write")]
        "function writeFile(self, key: string, data: buffer | string, options: { offset: number?, append: boolean?, create: boolean? }?): dream_vfs_VfsFile",
        #[cfg(any(feature = "lua-write", feature = "lua-host"))]
        "declare extern type dream_vfs_Writer with",
        #[cfg(feature = "lua-host")]
        "declare extern type dream_vfs_HostEntries with",
        #[cfg(feature = "lua-host")]
        "    host: { readFile: (path: string) -> buffer,",
        #[cfg(feature = "lua-write")]
        "fromDirectories: (dirs: { string }, options: { archives: { string }?, writeRoot: string? }?) -> dream_vfs_VFS",
        "function readInto(self, buffer: buffer, bufferOffset: number?, length: number?): number",
        "declare extern type dream_vfs_Keys with",
        "    [number]: string?",
        "    function toTable(self): { string }",
        "    [number]: { key: string, file: dream_vfs_VfsFile }?",
        "function __iter(self): (({}, number) -> (number?, { dir: string, file: dream_vfs_VfsFile }), {}, number)",
        "    VFS: { new: () -> dream_vfs_VFS,",
    ] {
        assert!(
            definitions.contains(expected),
            "{expected}\n---\n{definitions}"
        );
    }
    for fallback in [
        "(self, ...any): any",
        "(...any) -> ...any",
        ": any,\n",
        ": any\n",
    ] {
        assert!(
            !definitions.contains(fallback),
            "every member is typed ({fallback:?} found):\n{definitions}"
        );
    }
    check_strict_script(&plan, &strict_script());
}

/// A strict script that touches every part of the module's surface, built from the features on.
#[cfg(feature = "luau-analysis")]
fn strict_script() -> String {
    "--!strict\n\
         local vfstool = require('@dream/vfs')\n\
         local path = require('@dream/path')\n\
         local vfs: dream_vfs_VFS = vfstool.VFS.fromDirectories({ 'data' }, { archives = { 'a.bsa' } })\n\
         local other, conflicts = vfstool.VFS.fromDirectoriesWithConflictIndex({ 'data' })\n\
         local count: number = vfs:len() + other:len() + #conflicts:sources()\n\
         local file: dream_vfs_VfsFile? = vfs:getFile(path.normalize('Textures\\\\Foo.DDS'))\n\
         if file then\n\
             local bytes: string = file:readAll()\n\
             local written: number = file:readInto(buffer.create(64), 0)\n\
             local size: number = file:size()\n\
             local copied: number = file:readAt(buffer.create(64), 0, 16, 8)\n\
             local range: buffer = file:readRange(0, 16)\n\
             local whole: buffer = file:readAllBuffer()\n\
             local reader: dream_vfs_Reader = file:open()\n\
             local chunk: buffer = reader:read(4)\n\
             local got: number = reader:readInto(chunk, 0, 2)\n\
             reader:seek(0) reader:skip(1)\n\
             print(bytes, written, size, copied, range, whole, got, reader:tell(), reader:size(), file:path(), file:isLoose())\n\
             reader:close()\n\
         end\n\
         local keys: dream_vfs_Keys = vfs:keys()\n\
         local n: number = #keys\n\
         local first: string? = keys[1]\n\
         local joined: string = ''\n\
         for _, key in keys do joined ..= key end\n\
         local table: { string } = keys:toTable()\n\
         local entries: dream_vfs_Entries = vfs:pathsWith('textures')\n\
         local entry = entries[#entries]\n\
         if entry then local name: string = entry.key print(name, entry.file:isLoose()) end\n\
         for _, row in vfs:entries() do local key: string = row.key print(key, row.file:path()) end\n\
         local providers: dream_vfs_Providers? = vfs:providersFor('textures/foo.dds')\n\
         if providers then for _, p in providers do local kind: string = p.source.kind print(kind, p.file:path()) end end\n\
         local records: dream_vfs_ProviderRecords = vfs:providerRecordsFor('textures/foo.dds')\n\
         for _, record in records do local index: number = record.sourceIndex print(index, record.resolvedPath) end\n\
         local explain = vfs:explain('textures/foo.dds')\n\
         if explain then local winner: string = explain.winner.source.path print(explain.key, winner, #explain.overridden) end\n\
         local layer: dream_vfs_LayerIndex = vfs:layerIndex()\n\
         local lock: dream_vfs_VfsLock = layer:lockManifest(vfs)\n\
         local drift = layer:diffAgainstLock(vfs, lock)\n\
         local provenance = layer:provenance(vfs, 'textures/foo.dds', true)\n\
         if provenance then local size: integer? = provenance.providers[1].size print(size) end\n\
         local tree: dream_vfs_Tree = vfs:tree()\n\
         for _, row in tree do local dir: string = row.dir print(dir, row.file:fileName()) end\n\
         local nested: { [string]: any } = tree:toTable()\n\
         local written, snapshot = vfstool.runSetup(vfs, 'merged', false)\n\
         local copied = vfstool.runFinalize('merged', 'out', snapshot)\n\
         print(count, n, first, joined, table, nested, #drift.entries, vfs:contains('x'), written, #copied, tree:count())\n"
            .to_owned()
            + if cfg!(feature = "serialize") {
                "print(vfstool.serialize(keys, 'json'))\n"
            } else {
                ""
            }
        + if cfg!(feature = "lua-write") {
                "local out: dream_vfs_VFS = vfstool.VFS.fromDirectories({ 'data' }, { writeRoot = 'out' })\n\
                 local made: dream_vfs_VfsFile = out:writeFile('a/b.txt', buffer.create(4), { append = true })\n\
                 local w: dream_vfs_Writer = out:openWrite('c.txt', { truncate = false })\n\
                 local n: number = w:write('abc', 1, 2) + w:writeAt(0, buffer.create(2))\n\
                 w:seek(0) w:truncate(1) w:flush()\n\
                 local closed: dream_vfs_VfsFile? = w:close()\n\
                 out:mkdir('d') out:rename('c.txt', 'd/c.txt') out:remove('d')\n\
                 local root: string? = out:writeRoot()\n\
                 out:setWriteRoot('elsewhere')\n\
                 print(made:size(), n, w:tell(), closed, root)\n"
            } else {
                ""
            }
        + if cfg!(feature = "lua-host") {
            "local host = vfstool.host\n\
             local blob: buffer = host.readFile('a.bin')\n\
             local text: string = host.readFileString('a.txt')\n\
             local part: buffer = host.readAt('a.bin', 0, 4)\n\
             local wrote: number = host.writeFile('b.txt', blob, { append = true })\n\
             local hr: dream_vfs_Reader = host.open('a.bin')\n\
             local hw: dream_vfs_Writer = host.openWrite('c.txt', { append = false })\n\
             local st = host.stat('a.bin')\n\
             if st then local sz: number = st.size print(sz, st.isFile, st.isDir, st.modified, st.readonly) end\n\
             local rows: dream_vfs_HostEntries = host.list('.', { recursive = true })\n\
             for _, row in rows do local p: string = row.path print(p, row.isDir, row.size) end\n\
             host.mkdir('d', { recursive = true }) host.rename('d', 'e') host.remove('e', { recursive = true })\n\
             print(text, part, wrote, hr:size(), hw:tell(), host.exists('x'), host.copy('a.txt', 'b.txt'), host.canonicalize('.'), #rows)\n"
        } else {
            ""
        }
}

/// Type checks `script` in strict mode against the plan's definitions and module stubs.
#[cfg(feature = "luau-analysis")]
fn check_strict_script(plan: &Rc<RuntimePlan>, script: &str) {
    use l3i::analysis::{
        Analysis, AnalysisOptions, Definitions, Mode, ModuleConfig, SourceCode, SourceProvider,
    };
    struct Script(String);
    impl SourceProvider for Script {
        fn read_source(&self, name: &str) -> Option<SourceCode> {
            (name == "script").then(|| SourceCode {
                text: self.0.clone(),
                is_script: true,
            })
        }
        fn module_config(&self, _: &str) -> ModuleConfig {
            ModuleConfig {
                mode: Mode::Strict,
                ..ModuleConfig::default()
            }
        }
    }
    let options = AnalysisOptions {
        definitions: vec![Definitions {
            name: "dream.d.luau".to_owned(),
            source: plan.type_definitions(),
        }],
        ..AnalysisOptions::default()
    };
    let analysis = Analysis::new(plan.analysis_sources(Script(script.to_owned())), options)
        .expect("the analysis frontend accepts the definitions");
    let report = analysis.check("script", false);
    let text: Vec<String> = report
        .diagnostics
        .iter()
        .map(|d| {
            format!(
                "script:{}:{}: {}",
                d.span.begin_line + 1,
                d.span.begin_column + 1,
                d.text
            )
        })
        .collect();
    assert!(report.is_clean(), "{}", text.join("\n"));
}

#[test]
fn keys_and_entries_are_sequence_views_with_one_based_indices() {
    let dir = TempDir::new("views");
    for index in 0..8 {
        dir.write(&format!("dir{}/file{index}.txt", index % 3), b"");
    }
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir })
            assert(vfs:len() == 8 and not vfs:isEmpty())
            local keys = vfs:keys()
            assert(#keys == 8, 'len')
            assert(keys[1] == 'dir0/file0.txt' and keys[8] == 'dir2/file5.txt', 'sorted, 1-based')
            assert(keys[0] == nil and keys[9] == nil and keys[1.5] == nil, 'no element')
            local seen = 0
            for index, key in keys do seen += 1 assert(keys[index] == key) end
            assert(seen == 8, 'iterate')
            local plain = keys:toTable()
            assert(#plain == 8 and plain[2] == 'dir0/file3.txt', 'toTable')
            assert(not pcall(ipairs, keys), 'ipairs wants a table; use toTable()')
            local entries = vfs:entries()
            assert(#entries == 8 and entries[1].key == 'dir0/file0.txt' and entries[1].file:isLoose(), 'entries')
            assert(#vfs:pathsWith('dir1') == 3 and vfs:pathsWith('dir1')[1].key == 'dir1/file1.txt')
            assert(#vfs:pathsWith('DIR1/') == 3 and #vfs:pathsWith('dir') == 0, 'component boundaries')
            assert(#vfs:pathsMatching('file7') == 1 and #vfs:pathsMatching('FILE') == 8)
            local count = 0
            for _, row in vfs:tree() do count += 1 assert(row.dir:find('Data Files', 1, true) == 1 and row.file:isLoose()) end
            assert(count == 8 and vfs:tree():count() == 8, 'tree rows')
            local nested = vfs:tree():toTable()
            assert(#nested['Data Files'].subdirs.dir0.files == 3, 'nested shape')
            assert(#vfs:providersFor('dir0/file0.txt') == 1 and vfs:providersFor('nope') == nil)
            local records = vfs:providerRecordsFor('dir0/file0.txt')
            assert(#records == 1 and records[1].sourceIndex == 1 and records[1].key == 'dir0/file0.txt', '1-based source index')
            assert(vfs:explain('dir0/file0.txt').winner.sourceIndex == 1)
            local layer = vfs:layerIndex()
            assert(layer:sourcesContaining('dir0/file0.txt')[1] == 1 and layer:sourceIdForPath(dir) == 1)
            assert(layer:sourceById(1).path == dir and layer:sourceById(2) == nil)
            assert(not pcall(function() return layer:sourceById(0) end), 'zero is not an index')
            assert(layer:providerChain('dir0/file0.txt')[1].providerIndex == 1)
            assert(tostring(vfs) == 'dream.vfs.VFS(8 files)')
            -- The sorted key list is shared between calls and rebuilt after a mutation.
            local before = vfs:keys()
            assert(vfs:removeResolvedFile('dir0/file0.txt') ~= nil)
            local after = vfs:keys()
            assert(#before == 8 and #after == 7 and after[1] == 'dir0/file3.txt', 'keys follow mutation')
            ",
        )
        .unwrap();
}

#[test]
fn paths_are_bytes_and_read_into_fills_a_buffer() {
    let dir = TempDir::new("bytes");
    dir.write(
        "Data/Big.bin",
        &(0..=255u8).cycle().take(1000).collect::<Vec<u8>>(),
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let odd = dir.0.join(std::ffi::OsStr::from_bytes(b"odd/\xff.dat"));
        fs::create_dir_all(odd.parent().unwrap()).unwrap();
        fs::write(&odd, b"odd").unwrap();
    }
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir })
            local file = vfs:getFile('DATA\\BIG.BIN')
            assert(file, 'found in another spelling')
            assert(vfs:contains('data/big.bin'), 'contains: ' .. tostring(vfs:contains('data/big.bin')) .. ' len ' .. tostring(vfs:len()) .. ' keys ' .. table.concat(vfs:keys():toTable(), ','))
            assert(not vfs:contains('data/small.bin'), 'missing key')
            assert(#file:readAll() == 1000, 'readAll')
            assert(file:fileName() == 'Big.bin', file:fileName())
            assert(file:fileStem() == 'Big', file:fileStem())
            local buf = buffer.create(1000)
            local written = file:readInto(buf)
            assert(written == 1000, tostring(written))
            assert(buffer.readu8(buf, 255) == 255 and buffer.readu8(buf, 256) == 0, 'contents')
            local small = buffer.create(64)
            assert(file:readInto(small, 32) == 32, 'reads at most the space after the offset')
            assert(buffer.readu8(small, 32) == 0 and buffer.readu8(small, 63) == 31)
            assert(not pcall(file.readInto, file, small, 65), 'offset past the end')
            assert(not pcall(file.readInto, file, small, -1), 'negative offset')
            assert(not pcall(vfs.getFile, vfs, 42), 'a path is a string')
            assert(not pcall(vfstool.VfsFile.from('/definitely/missing').readAll, vfstool.VfsFile.from('/definitely/missing')))
            ",
        )
        .unwrap();
    #[cfg(unix)]
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir })
            local odd = vfs:getFile('ODD/\255.DAT')
            assert(odd and odd:readAll() == 'odd' and odd:fileName() == '\255.dat', 'a key that is not UTF-8 is a key')
            assert(vfs:keys()[2] == 'odd/\255.dat' and #vfs:pathsMatching('\255') == 1)
            assert(vfs:explain('odd/\255.dat').key == 'odd/\255.dat')
            ",
        )
        .unwrap();
}

#[test]
fn option_tables_are_strict_and_sizes_are_integers() {
    let dir = TempDir::new("options");
    dir.write("a.txt", b"abcd");
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    let error = runtime
        .exec("vfstool.VFS.fromDirectories({ dir }, { archive = { 'x.bsa' } })")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown option 'archive'"), "{error}");
    let error = runtime
        .exec("vfstool.VFS.new():collapseInto(dir, { allow_copying = true })")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown option 'allow_copying'"), "{error}");
    let error = runtime
        .exec("vfstool.VfsProvider.new({ path = dir, kind = 'weird' }, vfstool.VfsFile.from(dir))")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown source kind 'weird'"), "{error}");
    // A direct argument's type error names the argument slot the way l3i's own conversions do; a
    // value reached through a path names the path in full (`dirs[1]`, `fromFileLists[1].files[1]`,
    // `VfsProvider.new.path`) with Luau's own type names, and a nested reader's context is one
    // segment per level.
    runtime
        .exec(
            r"
            local ok, err = pcall(vfstool.VFS.fromDirectories, 42)
            assert(not ok and err:find('dirs: Lua stack index 1: expected table, got number', 1, true), err)
            ok, err = pcall(vfstool.VFS.fromDirectories, { 42 })
            assert(not ok and err:find('dirs[1]: expected string, got number', 1, true), err)
            ok, err = pcall(vfstool.VFS.fromDirectories, { dir }, { archives = 'a.bsa' })
            assert(not ok and err:find('VFS.fromDirectories.archives: expected table, got string', 1, true), err)
            ok, err = pcall(vfstool.VFS.fromDirectories, { dir }, { archives = { 1 } })
            assert(not ok and err:find('VFS.fromDirectories.archives[1]: expected string, got number', 1, true), err)
            ok, err = pcall(vfstool.VfsProvider.new, { path = 42, kind = 'looseDir' }, vfstool.VfsFile.from(dir))
            assert(not ok and err:find('VfsProvider.new.path: expected string, got number', 1, true), err)
            ok, err = pcall(vfstool.LayerIndex.fromFileLists, { { source = { path = dir, kind = 'looseDir' }, files = 'nope' } })
            assert(not ok and err:find('LayerIndex.fromFileLists[1].files: expected table, got string', 1, true), err)
            ok, err = pcall(vfstool.ConflictIndex.fromFileLists, { { source = dir, files = { 1 } } })
            assert(not ok and err:find('ConflictIndex.fromFileLists[1].files[1]: expected string, got number', 1, true), err)
            ok, err = pcall(vfstool.LayerIndex.fromFileLists, { { source = { path = dir, kind = 'looseDir' }, files = { 1 } } })
            assert(not ok and err:find('LayerIndex.fromFileLists[1].files[1]: expected string, got number', 1, true), err)
            ok, err = pcall(vfstool.LayerIndex.fromFileLists, { { source = { path = dir }, files = {} } })
            assert(not ok and err:find([[LayerIndex.fromFileLists[1].source: missing required option 'kind']], 1, true), err)
            ok, err = pcall(vfstool.LayerIndex.fromFileLists, 'rows')
            assert(not ok and err:find('expected table, got string', 1, true), err)
            ",
        )
        .unwrap();
    runtime
        .exec(
            r"
            local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex({ dir })
            local provenance = layer:provenance(vfs, 'a.txt', true)
            assert(provenance.providers[1].size == 4i, 'sizes are integers')
            assert(#provenance.providers[1].hashBlake3 == 64)
            local lock = layer:lockManifest(vfs)
            assert(lock:entries()[1].winnerSize == 4i and lock:toTable().entries[1].providerCount == 1)
            ",
        )
        .unwrap();
    #[cfg(feature = "serialize")]
    runtime
        .exec(
            r#"
            local vfs = vfstool.VFS.fromDirectories({ dir })
            assert(#vfstool.serialize({ files = vfs:keys(), n = 1, ok = true, nested = { 1, 2 } }, 'json') > 0)
            local json = vfstool.serialize({ files = vfs:keys() }, 'json')
            assert(json == '{"files":["a.txt"]}', json)
            assert(vfstool.serialize({}, 'json') == '[]')
            assert(not pcall(vfstool.serialize, vfs, 'json'), 'a VFS handle is not serializable')
            assert(not pcall(vfstool.serialize, {}, 'xml'), 'unknown format')
            "#,
        )
        .unwrap();
}

#[test]
fn semantic_conflicts_are_rows_of_plain_tables() {
    let low = TempDir::new("semantic_low");
    low.write("a.txt", b"same");
    low.write("b.json", b"{\"x\": 1}");
    let high = TempDir::new("semantic_high");
    high.write("a.txt", b"same");
    high.write("b.json", b"{ \"x\": 1 }");
    let runtime = runtime();
    runtime
        .exec(&format!("low = {:?} high = {:?}", low.lua(), high.lua()))
        .unwrap();
    // JSON is only parsed with `serialize`; without it the delta is unknown, as documented.
    let json_delta = if cfg!(feature = "serialize") {
        "cosmeticOnly"
    } else {
        "unknown"
    };
    runtime
        .exec(&format!("jsonDelta = {json_delta:?}"))
        .unwrap();
    runtime
        .exec(
            r#"
            local vfs, layer = vfstool.VFS.fromDirectoriesWithLayerIndex({ low, high })
            local report = layer:semanticConflicts(vfs, { includeSemanticDeltas = true, archiveHashMode = 'disabled' })
            assert(#report.entries == 2, #report.entries)
            local byKey = {}
            for _, entry in ipairs(report.entries) do byKey[entry.key] = entry end
            local a, b = byKey['a.txt'], byKey['b.json']
            assert(a.winner.path == high and a.winner.kind == 'looseDir', a.winner.path)
            assert(#a.providers == 2 and a.allIdentical == true and a.distinctVersions == 1)
            assert(a.providers[1].source.path == low and a.providers[1].relation == 'identicalToWinner')
            assert(a.providers[1].size == 4i and #a.providers[1].hashBlake3 == 64)
            assert(a.providers[2].relation == 'identicalToWinner' and a.providers[2].semanticDeltaToWinner.kind == 'noOpEquivalent')
            assert(b.allIdentical == false and b.distinctVersions == 2 and b.assetClass == 'json')
            assert(b.providers[1].relation == 'differentFromWinner')
            assert(b.providers[1].semanticDeltaToWinner.kind == jsonDelta, b.providers[1].semanticDeltaToWinner.kind)
            local ok, err = pcall(layer.semanticConflicts, layer, vfs, { archiveHashMode = 'sometimes' })
            assert(not ok and err:find("archiveHashMode", 1, true) and err:find("got 'sometimes'", 1, true), err)
            "#,
        )
        .unwrap();
}

#[test]
fn positional_reads_copy_from_a_map_and_name_their_arguments() {
    let dir = TempDir::new("positional");
    let payload: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    dir.write("data/big.bin", &payload);
    dir.write("data/empty.bin", b"");
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir })
            local file = vfs:getFile('data/big.bin')
            assert(file:size() == 1000, 'size from metadata')
            local buf = buffer.create(64)
            assert(file:readAt(buf, 0) == 64, 'fills the buffer by default')
            assert(buffer.readu8(buf, 0) == 0 and buffer.readu8(buf, 63) == 63)
            assert(file:readAt(buf, 100, 16, 8) == 16, 'length and bufferOffset')
            assert(buffer.readu8(buf, 8) == 100 and buffer.readu8(buf, 23) == 115 and buffer.readu8(buf, 24) == 24, 'only the window changed')
            assert(file:readAt(buf, 990) == 10, 'short at the end of the file')
            assert(file:readAt(buf, 1000) == 0, 'the end reads nothing')
            local ok, err = pcall(file.readAt, file, buf, 1001)
            assert(not ok and err:find('dream.vfs: readAt: fileOffset 1001 past the end (size 1000)', 1, true), err)
            ok, err = pcall(file.readAt, file, buf, -1)
            assert(not ok and err:find('dream.vfs: readAt: fileOffset -1 is negative', 1, true), err)
            ok, err = pcall(file.readAt, file, buf, 0, 65)
            assert(not ok and err:find('dream.vfs: readAt: length 65 does not fit the buffer (space 64 after bufferOffset 0)', 1, true), err)
            ok, err = pcall(file.readAt, file, buf, 0, 8, 60)
            assert(not ok and err:find('dream.vfs: readAt: length 8 does not fit the buffer (space 4 after bufferOffset 60)', 1, true), err)
            ok, err = pcall(file.readAt, file, buf, 0, nil, 65)
            assert(not ok and err:find('dream.vfs: readAt: bufferOffset 65 past the end of the buffer (size 64)', 1, true), err)
            ok, err = pcall(file.readAt, file, buf, 0, -3)
            assert(not ok and err:find('dream.vfs: readAt: length -3 is negative', 1, true), err)
            ok, err = pcall(file.readAt, file, buf, 1.5)
            assert(not ok and err:find('not an exact', 1, true), err)
            local range = file:readRange(250, 10)
            assert(buffer.len(range) == 10 and buffer.readu8(range, 0) == 250 and buffer.readu8(range, 9) == 3, 'readRange')
            assert(buffer.len(file:readRange(995, 100)) == 5, 'readRange is short at the end')
            assert(buffer.len(file:readRange(1000, 1)) == 0)
            ok, err = pcall(file.readRange, file, 1001, 1)
            assert(not ok and err:find('dream.vfs: readRange: fileOffset 1001 past the end (size 1000)', 1, true), err)
            ok, err = pcall(file.readRange, file, 0, -1)
            assert(not ok and err:find('dream.vfs: readRange: length -1 is negative', 1, true), err)
            local whole = file:readAllBuffer()
            assert(buffer.len(whole) == 1000 and buffer.readu8(whole, 999) == 231, 'readAllBuffer')
            assert(#file:readAll() == 1000 and file:readInto(buf) == 64, 'the old reads still work')
            local empty = vfs:getFile('data/empty.bin')
            assert(empty:size() == 0 and buffer.len(empty:readAllBuffer()) == 0 and empty:readAt(buf, 0) == 0, 'an empty file')
            local missing = vfstool.VfsFile.from(dir .. '/missing.bin')
            ok, err = pcall(missing.size, missing)
            assert(not ok and err:find('dream.vfs: ', 1, true), err)
            assert(not pcall(missing.readAt, missing, buf, 0))
            ",
        )
        .unwrap();
}

#[test]
fn readers_walk_a_file_sequentially_and_refuse_use_after_close() {
    let dir = TempDir::new("reader");
    let payload: Vec<u8> = (0..=255u8).cycle().take(300).collect();
    dir.write("a.bin", &payload);
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir })
            local file = vfs:getFile('a.bin')
            local reader = file:open()
            assert(reader:size() == 300 and reader:tell() == 0)
            assert(tostring(reader):find('dream.vfs.Reader(', 1, true) and tostring(reader):find('a.bin', 1, true), tostring(reader))
            local first = reader:read(16)
            assert(buffer.len(first) == 16 and buffer.readu8(first, 15) == 15 and reader:tell() == 16)
            local buf = buffer.create(32)
            assert(reader:readInto(buf) == 32 and buffer.readu8(buf, 0) == 16 and reader:tell() == 48)
            assert(reader:readInto(buf, 8, 4) == 4 and buffer.readu8(buf, 8) == 48 and buffer.readu8(buf, 12) == 28, 'window')
            reader:skip(200)
            assert(reader:tell() == 252)
            local tail = reader:read(100)
            assert(buffer.len(tail) == 48 and reader:tell() == 300, 'short at the end')
            assert(buffer.len(reader:read(10)) == 0, 'nothing left')
            reader:seek(10)
            assert(reader:tell() == 10 and buffer.readu8(reader:read(1), 0) == 10)
            reader:seek(300)
            assert(reader:readInto(buf) == 0)
            local ok, err = pcall(reader.seek, reader, 301)
            assert(not ok and err:find('dream.vfs: Reader.seek: position 301 past the end (size 300)', 1, true), err)
            ok, err = pcall(reader.seek, reader, -1)
            assert(not ok and err:find('dream.vfs: Reader.seek: position -1 is negative', 1, true), err)
            reader:seek(0)
            ok, err = pcall(reader.skip, reader, 301)
            assert(not ok and err:find('dream.vfs: Reader.skip: count 301 past the end (position 0, size 300)', 1, true), err)
            ok, err = pcall(reader.read, reader, -1)
            assert(not ok and err:find('dream.vfs: Reader.read: length -1 is negative', 1, true), err)
            ok, err = pcall(reader.readInto, reader, buf, 33)
            assert(not ok and err:find('dream.vfs: Reader.readInto: bufferOffset 33 past the end of the buffer (size 32)', 1, true), err)
            -- A second reader over the same file shares the map and has its own position.
            local other = file:open()
            assert(other:tell() == 0 and buffer.readu8(other:read(1), 0) == 0 and reader:tell() == 0)
            reader:close()
            reader:close()
            ok, err = pcall(reader.read, reader, 1)
            assert(not ok and err:find('dream.vfs: Reader.read: the reader over ', 1, true) and err:find('a.bin is closed', 1, true), err)
            ok, err = pcall(reader.seek, reader, 0)
            assert(not ok and err:find('is closed', 1, true), err)
            assert(reader:tell() == 0 and reader:size() == 300, 'tell and size still answer')
            assert(buffer.readu8(other:read(1), 0) == 1, 'the other reader is unaffected')
            -- A sequential parse: every chunk of the file, in order, through one buffer.
            local walk = file:open()
            local chunk = buffer.create(64)
            local total, checksum = 0, 0
            while true do
                local n = walk:readInto(chunk)
                if n == 0 then break end
                for i = 0, n - 1 do checksum += buffer.readu8(chunk, i) end
                total += n
            end
            assert(total == 300 and checksum == 255 * 128 + 44 * 43 / 2, tostring(checksum))
            ",
        )
        .unwrap();
}

#[cfg(feature = "zip")]
#[test]
fn zip_members_read_positionally_from_the_index_size() {
    use std::io::Write as _;
    // Test builds cap a buffered zip entry at 64 bytes, so the entries stay under that.
    let dir = TempDir::new("zip_reads");
    let payload: Vec<u8> = (0..60u8).collect();
    let path = dir.0.join("Data/Extras.zip");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("textures/stored.bin", stored).unwrap();
    zip.write_all(&payload).unwrap();
    zip.start_file("textures/packed.bin", deflated).unwrap();
    zip.write_all(&payload).unwrap();
    zip.finish().unwrap();
    let runtime = runtime();
    runtime
        .exec(&format!("dir = {:?}", dir.0.join("Data").to_string_lossy()))
        .unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir }, { archives = { 'Extras.zip' } })
            for _, name in { 'textures/stored.bin', 'textures/packed.bin' } do
                local file = vfs:getFile(name)
                assert(file:isArchive() and file:size() == 60, name)
                local buf = buffer.create(16)
                assert(file:readAt(buf, 16) == 16 and buffer.readu8(buf, 0) == 16 and buffer.readu8(buf, 15) == 31, name)
                assert(file:readAt(buf, 50) == 10, 'short at the end')
                local ok, err = pcall(file.readAt, file, buf, 61)
                assert(not ok and err:find('fileOffset 61 past the end (size 60)', 1, true), err)
                local range = file:readRange(20, 8)
                assert(buffer.len(range) == 8 and buffer.readu8(range, 0) == 20, name)
                assert(buffer.len(file:readAllBuffer()) == 60)
                local reader = file:open()
                reader:seek(40)
                assert(buffer.readu8(reader:read(1), 0) == 40 and reader:tell() == 41, name)
                assert(file:readAll() == buffer.tostring(file:readAllBuffer()), 'the two whole reads agree')
            end
            ",
        )
        .unwrap();
}

#[cfg(feature = "beth-archives")]
#[test]
fn bsa_members_read_positionally() {
    let dir = TempDir::new("bsa_reads");
    let payload: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    let data = dir.0.join("Data");
    fs::create_dir_all(&data).unwrap();
    let mut builder = dream_archive::Tes3BsaBuilder::new();
    builder.add_bytes("meshes\\x\\door.nif", &payload).unwrap();
    builder.write_path(data.join("Morrowind.bsa")).unwrap();
    let runtime = runtime();
    runtime
        .exec(&format!("dir = {:?}", data.to_string_lossy()))
        .unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ dir }, { archives = { 'Morrowind.bsa' } })
            local file = vfs:getFile('meshes/x/door.nif')
            assert(file and file:isArchive() and file:size() == 3000)
            local buf = buffer.create(8)
            assert(file:readAt(buf, 512) == 8 and buffer.readu8(buf, 0) == 0 and buffer.readu8(buf, 7) == 7)
            assert(file:readAt(buf, 2996) == 4)
            assert(buffer.len(file:readRange(2990, 100)) == 10)
            local reader = file:open()
            reader:skip(2999)
            assert(buffer.readu8(reader:read(4), 0) == 2999 % 256 and reader:tell() == 3000)
            assert(buffer.len(file:readAllBuffer()) == 3000)
            ",
        )
        .unwrap();
}

// ---------------------------------------------------------------------------------------------
// require over the VFS
// ---------------------------------------------------------------------------------------------

/// A runtime, its sandbox, and `require` over `vfs` through `cache`.
fn runtime_requiring(
    vfs: &super::Vfs,
    cache: &Rc<super::TemplateCache>,
) -> (Rc<Runtime>, Rc<l3i::sandbox::Sandbox>) {
    let runtime = Rc::new(runtime());
    let sandbox = Rc::new(
        runtime
            .sandbox(|_| {}, l3i::sandbox::SandboxOptions::default())
            .unwrap(),
    );
    runtime
        .install_require(
            super::VfsRequireNavigator::with_cache(vfs.clone(), sandbox.clone(), cache.clone())
                .alias("dream", "scripts/dream"),
        )
        .unwrap();
    (runtime, sandbox)
}

/// Runs the VFS module at `key` as the chunk `@key` in the globals and returns its first result.
fn run_module<R: l3i::call::CallResults>(
    runtime: &Runtime,
    vfs: &super::Vfs,
    key: &str,
) -> l3i::Result<R> {
    let source = vfs
        .with(|vfs| vfs.get_file(key).map(|file| file.read_to_vec().unwrap()))
        .unwrap()
        .unwrap_or_else(|| panic!("{key} is in the VFS"));
    let options = runtime.compile_options();
    let stack = runtime.stack();
    stack.with_frame(|frame| {
        let chunk = runtime.load(
            frame,
            &format!("@{key}"),
            std::str::from_utf8(&source).unwrap(),
            &options,
        )?;
        chunk.as_function()?.invoke::<R, ()>(frame, ())
    })
}

#[test]
fn require_resolves_relative_paths_init_modules_and_aliases_through_the_vfs() {
    let dir = TempDir::new("require");
    dir.write("scripts/main.luau", b"local greet = require('./lib/greet') local pkg = require('./pkg') local ui = require('@dream/ui') local m = require('@lib/math') local util = require('./util') return greet.four + pkg.value + ui.width + m.twice(1) + util.one");
    dir.write(
        "scripts/lib/math.luau",
        b"return { twice = function(x) return x * 2 end }",
    );
    dir.write(
        "scripts/lib/greet.lua",
        b"local math = require('./math') return { four = math.twice(2) }",
    );
    dir.write(
        "scripts/pkg/init.luau",
        b"local util = require('./util') return { value = 100 + util.one }",
    );
    dir.write("scripts/util.luau", b"return { one = 1 }");
    dir.write("scripts/dream/ui.luau", b"return { width = 1000 }");
    dir.write("scripts/.luaurc", br#"{ "aliases": { "lib": "./lib" } }"#);
    dir.write("scripts/both.luau", b"return 1");
    dir.write("scripts/both.lua", b"return 2");
    dir.write("scripts/bad.luau", b"return '\xff'");
    let vfs = super::Vfs::new(crate::VFS::from_directories([&dir.0], None));
    let cache = Rc::new(super::TemplateCache::new());
    let (runtime, _sandbox) = runtime_requiring(&vfs, &cache);
    let total: i32 = run_module(&runtime, &vfs, "scripts/main.luau").unwrap();
    assert_eq!(total, 4 + 101 + 1000 + 2 + 1);
    // The five modules compiled once each, whatever path reached them.
    assert_eq!(cache.compiles(), 5, "{cache:?}");
    assert_eq!(cache.len(), 5);
    let again: i32 = run_module(&runtime, &vfs, "scripts/main.luau").unwrap();
    assert_eq!(again, total);
    assert_eq!(
        cache.compiles(),
        5,
        "the second run is served from Luau's own cache: {cache:?}"
    );
    // Errors: a missing module, an ambiguous one, one that is not UTF-8, and a bad prefix.
    for (source, expected) in [
        ("return require('./nothing')", "no module present"),
        ("return require('./both')", "ambiguous"),
        ("return require('./bad')", "is not UTF-8"),
        (
            "return require('lib/math')",
            "must start with a valid prefix",
        ),
        ("return require('@nope/x')", "nope"),
    ] {
        let error = runtime
            .stack()
            .with_frame(|frame| {
                let chunk = runtime.load(
                    frame,
                    "@scripts/main.luau",
                    source,
                    &runtime.compile_options(),
                )?;
                chunk.as_function()?.invoke::<i32, ()>(frame, ())
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{source}: {error}");
    }
    // A chunk that is not a VFS key requires from the root; @dream/vfs still resolves.
    runtime
        .exec("assert(require('./scripts/util').one == 1) assert(require('@dream/vfs').VFS.new():len() == 0)")
        .unwrap();
    assert!(cache.invalidate(b"Scripts\\Util.luau") && !cache.invalidate(b"scripts/util.luau"));
    assert_eq!(cache.len(), 4);
    cache.clear();
    assert!(cache.is_empty());
}

/// A runtime whose sandbox instances share Luau's `require` over a counter module, with the
/// template cache in view.
struct CounterFixture {
    dir: TempDir,
    vfs: super::Vfs,
    cache: Rc<super::TemplateCache>,
    runtime: Rc<Runtime>,
    sandbox: Rc<l3i::sandbox::Sandbox>,
    instances: Vec<l3i::sandbox::Instance>,
    script: l3i::sandbox::Template,
}

impl CounterFixture {
    fn new() -> Self {
        let dir = TempDir::new("require_cache");
        dir.write(
            "scripts/counter.luau",
            b"local M = { n = 0 } function M.bump() M.n += 1 return M.n end return M",
        );
        let vfs = super::Vfs::new(crate::VFS::from_directories([&dir.0], None));
        let cache = Rc::new(super::TemplateCache::new());
        let (runtime, sandbox) = runtime_requiring(&vfs, &cache);
        let loader = runtime
            .bind_function("dream.vfs.tests.loader", |name: &str| -> l3i::Result<()> {
                Err(l3i::Error::runtime(format!("no package '{name}'")))
            })
            .unwrap();
        let require = runtime.global("require").unwrap();
        let instances = ["first", "second", "third"]
            .into_iter()
            .map(|name| {
                let spec = l3i::sandbox::InstanceSpec {
                    name,
                    packages: &[],
                    hidden_data: None,
                    loader: &loader,
                };
                let instance = sandbox.new_instance(&runtime, &spec).unwrap();
                instance
                    .env
                    .set(&runtime.stack(), "require", &require)
                    .unwrap();
                instance
            })
            .collect();
        let script = sandbox
            .load_template(
                &runtime,
                "@scripts/instance.luau",
                "return require('./counter').bump()",
            )
            .unwrap();
        CounterFixture {
            dir,
            vfs,
            cache,
            runtime,
            sandbox,
            instances,
            script,
        }
    }

    /// Runs the entry script in instance `index` and returns the counter it saw.
    fn run(&self, index: usize) -> l3i::Result<i32> {
        let results = self.sandbox.run(
            &self.runtime,
            &self.script,
            &self.instances[index],
            self.runtime.initialization_context(),
        )?;
        results[0].with_value(&self.runtime.stack(), |_, view| view.read::<i32>())
    }

    fn rewrite(&self, source: &str) {
        self.dir.write("scripts/counter.luau", source.as_bytes());
        assert!(
            self.vfs
                .with(|vfs| vfs.contains("scripts/counter.luau"))
                .unwrap()
        );
    }
}

/// With the `jit` feature forwarded, the cache reports how each template's native compilation
/// went, once, from the template it holds.
#[cfg(feature = "jit")]
#[test]
fn the_template_cache_reports_native_code_status_under_jit() {
    use l3i::extension::NativeCodePolicy;
    use l3i::native_code::{NativeCodeMode, NativeCodeStatus};

    let dir = TempDir::new("require_native");
    dir.write(
        "scripts/counter.luau",
        b"local M = { n = 0 } function M.bump() M.n += 1 return M.n end return M",
    );
    let vfs = super::Vfs::new(crate::VFS::from_directories([&dir.0], None));
    let cache = Rc::new(super::TemplateCache::new());
    let policy = RuntimePolicy::new()
        .compat_global(MODULE, MODULE_NAME)
        .native_code(NativeCodePolicy {
            mode: NativeCodeMode::Eager,
            ..NativeCodePolicy::default()
        });
    let plan = RuntimePlan::builder()
        .policy(policy)
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()
        .expect("the extension finalizes");
    let runtime = Rc::new(Runtime::from_plan(&plan).expect("a runtime"));
    let available = runtime
        .native_code()
        .is_some_and(l3i::native_code::NativeCodeGen::is_available);
    let sandbox = Rc::new(
        runtime
            .sandbox(
                |_| {},
                l3i::sandbox::SandboxOptions {
                    compile_options: runtime.compile_options(),
                    ..l3i::sandbox::SandboxOptions::default()
                },
            )
            .unwrap(),
    );
    runtime
        .install_require(super::VfsRequireNavigator::with_cache(
            vfs.clone(),
            sandbox.clone(),
            cache.clone(),
        ))
        .unwrap();
    assert!(
        cache.native_code(b"scripts/counter.luau").is_none(),
        "nothing loaded yet"
    );
    let loader = runtime
        .bind_function("dream.vfs.tests.loader", |name: &str| -> l3i::Result<()> {
            Err(l3i::Error::runtime(format!("no package '{name}'")))
        })
        .unwrap();
    let require = runtime.global("require").unwrap();
    let spec = l3i::sandbox::InstanceSpec {
        name: "native",
        packages: &[],
        hidden_data: None,
        loader: &loader,
    };
    let instance = sandbox.new_instance(&runtime, &spec).unwrap();
    instance
        .env
        .set(&runtime.stack(), "require", &require)
        .unwrap();
    let script = sandbox
        .load_template(
            &runtime,
            "@scripts/instance.luau",
            "return require('./counter').bump()",
        )
        .unwrap();
    sandbox
        .run(
            &runtime,
            &script,
            &instance,
            runtime.initialization_context(),
        )
        .unwrap();
    let status = cache.native_code(b"scripts/counter.luau");
    if available {
        assert_eq!(
            status.map(|result| result.status),
            Some(NativeCodeStatus::Success),
            "the module compiled natively once, at load: {status:?}"
        );
    } else {
        assert!(status.is_none(), "no generator on this platform");
    }
    assert!(
        cache.native_code(b"scripts/missing.luau").is_none(),
        "unknown keys have no status"
    );
}

#[test]
fn require_clones_one_template_per_instance() {
    let fixture = CounterFixture::new();
    // Two sandbox instances share Luau's require, and so the module table, until the host
    // clears Luau's result cache between them; the template is cloned, never recompiled.
    assert_eq!(fixture.run(0).unwrap(), 1);
    assert_eq!(
        fixture.run(1).unwrap(),
        2,
        "Luau caches the module result per VM"
    );
    fixture.runtime.clear_require_cache().unwrap();
    assert_eq!(
        fixture.run(2).unwrap(),
        1,
        "a fresh clone of the template, in the third instance"
    );
    assert_eq!(
        fixture.cache.compiles(),
        1,
        "one compile, three instances: {:?}",
        fixture.cache
    );
    // A module's error reaches the requiring script.
    fixture.rewrite("error('boom')");
    fixture.cache.clear();
    fixture.runtime.clear_require_cache().unwrap();
    let error = fixture.run(0).unwrap_err().to_string();
    assert!(error.contains("boom"), "{error}");
    assert_eq!(
        fixture.cache.len(),
        1,
        "the template stays; only running it failed"
    );
}

#[test]
fn require_reloads_a_rewritten_module_only_when_told() {
    let fixture = CounterFixture::new();
    assert_eq!(fixture.run(0).unwrap(), 1);
    // Rewritten on disk: still the template, until the cache is cleared.
    fixture.rewrite("return { bump = function() return 42 end }");
    fixture.runtime.clear_require_cache().unwrap();
    assert_eq!(
        fixture.run(0).unwrap(),
        1,
        "no fingerprint: the old template serves"
    );
    assert_eq!(fixture.cache.compiles(), 1);
    fixture.cache.clear();
    fixture.runtime.clear_require_cache().unwrap();
    assert_eq!(fixture.run(1).unwrap(), 42, "reloaded lazily after clear");
    assert_eq!(fixture.cache.compiles(), 2);
    assert_eq!(fixture.cache.len(), 1);
    // invalidate does the same for one key.
    fixture.rewrite("return { bump = function() return 7 end }");
    fixture.runtime.clear_require_cache().unwrap();
    assert!(fixture.cache.invalidate(b"scripts/counter.luau"));
    assert_eq!(fixture.run(2).unwrap(), 7);
    assert_eq!(fixture.cache.compiles(), 3);
}

// ---------------------------------------------------------------------------------------------
// Writes under a write root (lua-write)
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "lua-write")]
#[test]
fn writes_go_under_the_write_root_and_register_winners() {
    let data = TempDir::new("write_data");
    data.write("textures/a.dds", b"old");
    let out = TempDir::new("write_root");
    let runtime = runtime();
    runtime
        .exec(&format!("data = {:?} out = {:?}", data.lua(), out.lua()))
        .unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ data }, { writeRoot = out })
            assert(vfs:writeRoot() == out)
            local file = vfs:writeFile('textures/a.dds', 'new')
            assert(file:isLoose() and file:path() == out .. '/textures/a.dds', file:path())
            assert(file:readAll() == 'new' and vfs:getFile('textures/a.dds'):readAll() == 'new', 'later reads see the write')
            assert(vfs:providersFor('textures/a.dds')[1].source.path == out .. '/textures', 'the write root directory is the source')
            vfs:writeFile('new/deep/b.bin', buffer.fromstring('bufferdata'))
            assert(vfs:getFile('new/deep/b.bin'):readAll() == 'bufferdata', 'a buffer, into new directories')
            vfs:writeFile('textures/a.dds', 'XY', { offset = 1 })
            assert(vfs:getFile('textures/a.dds'):readAll() == 'nXY', 'offset writes in place')
            vfs:writeFile('textures/a.dds', 'Z', { append = true })
            assert(vfs:getFile('textures/a.dds'):readAll() == 'nXYZ', 'append')
            local ok, err = pcall(vfs.writeFile, vfs, 'missing.txt', 'x', { create = false })
            assert(not ok and err:find('dream.vfs: ', 1, true), err)
            ok, err = pcall(vfs.writeFile, vfs, 'x.txt', 'x', { offset = 0, append = true })
            assert(not ok and err:find('vfs:writeFile: offset and append cannot be combined', 1, true), err)
            ok, err = pcall(vfs.writeFile, vfs, 'x.txt', 'x', { bogus = true })
            assert(not ok and err:find([[vfs:writeFile: unknown option 'bogus']], 1, true), err)
            ok, err = pcall(vfs.writeFile, vfs, 'x.txt', 'x', { offset = -1 })
            assert(not ok and err:find('offset -1 is negative', 1, true), err)
            for _, bad in { '../x', '/abs', 'C:\\x', 'a/../b', '', 'a\\..\\b', 'a/./b' } do
                ok, err = pcall(vfs.writeFile, vfs, bad, 'x')
                assert(not ok and err:find('escapes the write root', 1, true), bad .. ': ' .. tostring(err))
                ok, err = pcall(vfs.mkdir, vfs, bad)
                assert(not ok and err:find('escapes the write root', 1, true), bad .. ': ' .. tostring(err))
            end
            ok, err = pcall(vfs.writeFile, vfs, 'textures', 'x')
            assert(not ok and err:find([[the VFS refused key 'textures']], 1, true), err)
            -- A writer: buffered, positional, registered on close only.
            local w = vfs:openWrite('log.txt')
            assert(tostring(w) == 'dream.vfs.Writer(' .. out .. '/log.txt)', tostring(w))
            assert(w:write('hello ') == 6 and w:tell() == 6)
            assert(w:write(buffer.fromstring('xxworldxx'), 2, 5) == 5 and w:tell() == 11)
            assert(w:writeAt(0, 'J') == 1 and w:tell() == 11, 'writeAt does not move')
            w:seek(6) w:write('W')
            w:truncate(5)
            assert(w:tell() == 5, 'a position past the new end moves to it')
            w:flush()
            assert(not vfs:contains('log.txt'), 'registered on close only')
            local closed = w:close()
            assert(closed and closed:readAll() == 'Jello' and vfs:getFile('log.txt'):readAll() == 'Jello', closed:readAll())
            assert(w:close() == nil, 'closing again is nothing')
            ok, err = pcall(w.write, w, 'x')
            assert(not ok and err:find('dream.vfs: Writer.write: the writer over ', 1, true) and err:find('log.txt is closed', 1, true), err)
            ok, err = pcall(w.write, w, 'x', 5)
            assert(not ok and err:find('dream.vfs: Writer.write: offset 5 past the end of the data (size 1)', 1, true), err)
            local a = vfs:openWrite('log.txt', { append = true })
            assert(a:tell() == 5, 'an append writer starts at the end')
            a:write('!')
            ok, err = pcall(a.writeAt, a, 0, 'x')
            assert(not ok and err:find('appends, so it cannot write at a position', 1, true), err)
            a:close()
            assert(vfs:getFile('log.txt'):readAll() == 'Jello!')
            local k = vfs:openWrite('log.txt', { truncate = false })
            k:write('H') k:close()
            assert(vfs:getFile('log.txt'):readAll() == 'Hello!')
            ok, err = pcall(vfs.openWrite, vfs, 'log.txt', { mode = 'w' })
            assert(not ok and err:find([[vfs:openWrite: unknown option 'mode']], 1, true), err)
            -- A writer dropped without close still flushes, and registers nothing.
            local dropped = vfs:openWrite('dropped.txt')
            dropped:write('bytes')
            dropped = nil
            ",
        )
        .unwrap();
    runtime.collect_garbage();
    assert_eq!(fs::read(out.0.join("dropped.txt")).unwrap(), b"bytes");
    runtime
        .exec("assert(not vfstool.VFS.fromDirectories({ out }):contains('dropped.txt') == false)")
        .unwrap();
}

#[cfg(feature = "lua-write")]
#[test]
fn directory_writes_stay_under_the_root_and_move_the_winners() {
    let data = TempDir::new("write_dirs_data");
    data.write("textures/a.dds", b"old");
    let out = TempDir::new("write_dirs_root");
    let runtime = runtime();
    runtime
        .exec(&format!("data = {:?} out = {:?}", data.lua(), out.lua()))
        .unwrap();
    runtime
        .exec(
            r"
            local vfs = vfstool.VFS.fromDirectories({ data }, { writeRoot = out })
            vfs:writeFile('log.txt', 'Hello!')
            local ok, err
            -- Directories, moves and removals stay under the root and follow the winners.
            vfs:mkdir('made/dir')
            vfs:rename('log.txt', 'moved/log.txt')
            assert(not vfs:contains('log.txt') and vfs:getFile('moved/log.txt'):readAll() == 'Hello!')
            assert(vfs:getFile('moved/log.txt'):path() == out .. '/moved/log.txt')
            vfs:remove('moved/log.txt')
            assert(not vfs:contains('moved/log.txt'))
            ok, err = pcall(vfs.remove, vfs, 'textures/nope.dds')
            assert(not ok and err:find([[dream.vfs: remove: 'textures/nope.dds' is not under the write root]], 1, true), err)
            ok, err = pcall(vfs.rename, vfs, 'nope.txt', 'x.txt')
            assert(not ok and err:find([[dream.vfs: rename: 'nope.txt' is not under the write root]], 1, true), err)
            vfs:writeFile('tmp/a.txt', 'a') vfs:writeFile('tmp/b/c.txt', 'c')
            vfs:remove('tmp')
            assert(not vfs:contains('tmp/a.txt') and not vfs:contains('tmp/b/c.txt'), 'a directory takes its winners with it')
            vfs:writeFile('d1/x.txt', 'x') vfs:writeFile('d1/sub/y.txt', 'y')
            vfs:rename('d1', 'd2')
            assert(not vfs:contains('d1/x.txt') and vfs:getFile('d2/x.txt'):path() == out .. '/d2/x.txt')
            assert(vfs:getFile('d2/sub/y.txt'):readAll() == 'y')
            -- Without a write root nothing is written.
            local plain = vfstool.VFS.fromDirectories({ data })
            assert(plain:writeRoot() == nil)
            ok, err = pcall(plain.writeFile, plain, 'a.txt', 'b')
            assert(not ok and err:find('dream.vfs: writeFile: this VFS has no write root', 1, true), err)
            plain:setWriteRoot(out .. '/later')
            assert(plain:writeFile('a.txt', 'b'):path() == out .. '/later/a.txt')
            ",
        )
        .unwrap();
    assert!(out.0.join("made/dir").is_dir());
    assert!(!out.0.join("tmp").exists() && !out.0.join("d1").exists());
}

#[cfg(not(feature = "lua-write"))]
#[test]
fn a_runtime_without_lua_write_has_no_write_methods() {
    let dir = TempDir::new("no_write");
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    runtime
        .exec(
            r"
            local ok, err = pcall(vfstool.VFS.fromDirectories, { dir }, { writeRoot = dir })
            assert(not ok and err:find([[unknown option 'writeRoot']], 1, true), err)
            local vfs = vfstool.VFS.fromDirectories({ dir })
            for _, name in { 'writeFile', 'openWrite', 'writeRoot', 'setWriteRoot', 'mkdir', 'remove', 'rename' } do
                assert(not pcall(function() return vfs[name](vfs, 'a', 'b') end), name)
            end
            ",
        )
        .unwrap();
}

// ---------------------------------------------------------------------------------------------
// Host-path I/O (lua-host)
// ---------------------------------------------------------------------------------------------

#[cfg(feature = "lua-host")]
#[test]
fn host_io_reads_writes_lists_and_moves_host_paths() {
    let dir = TempDir::new("host");
    let payload: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    dir.write("in/big.bin", &payload);
    dir.write("in/sub/small.txt", b"small");
    let runtime = runtime();
    runtime.exec(&format!("dir = {:?}", dir.lua())).unwrap();
    runtime
        .exec(
            r"
            local host = vfstool.host
            local whole = host.readFile(dir .. '/in/big.bin')
            assert(buffer.len(whole) == 1000 and buffer.readu8(whole, 999) == 231)
            assert(host.readFileString(dir .. '/in/sub/small.txt') == 'small')
            local part = host.readAt(dir .. '/in/big.bin', 256, 8)
            assert(buffer.len(part) == 8 and buffer.readu8(part, 0) == 0 and buffer.readu8(part, 7) == 7)
            assert(buffer.len(host.readAt(dir .. '/in/big.bin', 996, 100)) == 4, 'short at the end')
            local ok, err = pcall(host.readAt, dir .. '/in/big.bin', 1001, 1)
            assert(not ok and err:find('dream.vfs: host.readAt: offset 1001 past the end (size 1000)', 1, true), err)
            ok, err = pcall(host.readFile, dir .. '/nope.bin')
            assert(not ok and err:find('dream.vfs: ', 1, true), err)
            assert(host.writeFile(dir .. '/out.txt', 'abc') == 3)
            assert(host.writeFile(dir .. '/out.txt', buffer.fromstring('def'), { append = true }) == 3)
            assert(host.readFileString(dir .. '/out.txt') == 'abcdef')
            ok, err = pcall(host.writeFile, dir .. '/out.txt', 'x', { offset = 1 })
            assert(not ok and err:find([[host.writeFile: unknown option 'offset']], 1, true), err)
            local st = host.stat(dir .. '/out.txt')
            assert(st.size == 6 and st.isFile and not st.isDir and st.readonly == false and type(st.modified) == 'number', tostring(st.size))
            assert(host.stat(dir .. '/in').isDir and host.stat(dir .. '/in').size == 0)
            assert(host.stat(dir .. '/nope') == nil)
            assert(host.exists(dir .. '/out.txt') and not host.exists(dir .. '/nope'))
            local rows = host.list(dir .. '/in')
            assert(#rows == 2 and rows[1].path == dir .. '/in/big.bin' and rows[1].size == 1000 and not rows[1].isDir, rows[1].path)
            assert(rows[2].path == dir .. '/in/sub' and rows[2].isDir and rows[2].size == 0)
            local deep = host.list(dir .. '/in', { recursive = true })
            assert(#deep == 3 and deep[3].path == dir .. '/in/sub/small.txt' and deep[3].size == 5, deep[3].path)
            local names = {}
            for _, row in deep do names[#names + 1] = row.path end
            assert(#names == 3 and #deep:toTable() == 3)
            ok, err = pcall(host.list, dir .. '/in', { deep = true })
            assert(not ok and err:find([[host.list: unknown option 'deep']], 1, true), err)
            host.mkdir(dir .. '/a/b/c', { recursive = true })
            assert(host.stat(dir .. '/a/b/c').isDir)
            ok, err = pcall(host.mkdir, dir .. '/x/y')
            assert(not ok, 'mkdir without recursive needs the parent')
            host.mkdir(dir .. '/x')
            host.rename(dir .. '/out.txt', dir .. '/x/out.txt')
            assert(not host.exists(dir .. '/out.txt') and host.readFileString(dir .. '/x/out.txt') == 'abcdef')
            assert(host.copy(dir .. '/x/out.txt', dir .. '/x/copy.txt') == 6)
            assert(host.readFileString(dir .. '/x/copy.txt') == 'abcdef')
            host.remove(dir .. '/x/copy.txt')
            assert(not host.exists(dir .. '/x/copy.txt'))
            ok, err = pcall(host.remove, dir .. '/x')
            assert(not ok, 'a directory with content needs recursive')
            host.remove(dir .. '/x', { recursive = true })
            assert(not host.exists(dir .. '/x'))
            assert(host.canonicalize(dir .. '/in/sub/../big.bin') == host.canonicalize(dir) .. '/in/big.bin')
            local reader = host.open(dir .. '/in/big.bin')
            assert(reader:size() == 1000)
            reader:seek(500)
            assert(buffer.readu8(reader:read(1), 0) == 500 % 256 and reader:tell() == 501)
            reader:close()
            local w = host.openWrite(dir .. '/written.bin')
            assert(w:write(buffer.fromstring('0123456789')) == 10)
            w:writeAt(0, 'AB')
            w:truncate(4)
            assert(w:close() == nil, 'a host writer registers nothing')
            assert(host.readFileString(dir .. '/written.bin') == 'AB23')
            local a = host.openWrite(dir .. '/written.bin', { append = true })
            a:write('!') a:close()
            assert(host.readFileString(dir .. '/written.bin') == 'AB23!')
            ",
        )
        .unwrap();
    #[cfg(unix)]
    runtime
        .exec(
            r"
            local host = vfstool.host
            host.writeFile(dir .. '/\255.bin', 'odd')
            assert(host.readFileString(dir .. '/\255.bin') == 'odd', 'a host path that is not UTF-8')
            assert(host.stat(dir .. '/\255.bin').size == 3)
            ",
        )
        .unwrap();
}

#[cfg(not(feature = "lua-host"))]
#[test]
fn a_runtime_without_lua_host_has_no_host_table() {
    runtime().exec("assert(vfstool.host == nil)").unwrap();
}
