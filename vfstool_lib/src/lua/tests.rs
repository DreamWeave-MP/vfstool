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
    check_strict_script(
        &plan,
        &("--!strict\n\
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
            }),
    );
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
