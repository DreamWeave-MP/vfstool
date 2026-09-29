// SPDX-License-Identifier: MIT OR Apache-2.0
//! The `@dream/vfs` module end to end: the scripts the previous binding's tests ran, adapted
//! where the documented breaks require (sequence views instead of tables, 1-based indices).

#![cfg(feature = "lua")]

use std::{fs, path::PathBuf, rc::Rc};

use l3i::{
    Runtime,
    extension::{RuntimePlan, RuntimePolicy},
};
use vfstool_lib::lua::{MODULE, MODULE_NAME, VfsExtension};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "{name}_{}_{}",
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
        .unwrap()
}

fn runtime_with(globals: &[(&str, &str)]) -> Runtime {
    let runtime = Runtime::from_plan(&plan()).unwrap();
    for (name, value) in globals {
        runtime.exec(&format!("{name} = {value:?}")).unwrap();
    }
    runtime
}

#[test]
fn lua_vfs_provider_reports_and_layer_workflows() {
    let low = TempDir::new("lua_vfs_low");
    let high = TempDir::new("lua_vfs_high");
    low.write("Textures/Foo.DDS", b"same");
    high.write("textures/foo.dds", b"same");
    high.write("meshes/bar.nif", b"mesh");

    let runtime = runtime_with(&[("low", &low.lua()), ("high", &high.lua())]);
    runtime
        .exec(
            r#"
        local vfs, conflict = vfstool.VFS.fromDirectoriesWithConflictIndex({ low, high })
        assert(vfs:len() == 2)
        assert(vfs:contains("TEXTURES\\FOO.DDS"))
        assert(vfs:getFile("textures/foo.dds"):isLoose())
        assert(vfs:getFile("meshes/bar.nif"):readAll() == "mesh")
        assert(vfs:pathsMatching("textures")[1].key == "textures/foo.dds")
        local keys = vfs:keys()
        assert(#keys == 2 and keys[1] == "meshes/bar.nif" and keys[2] == "textures/foo.dds")
        assert(#vfs:pathsWith("textures") == 1)
        assert(#vfs:pathsWith("textures/") == 1)
        assert(#vfs:pathsWith("Textures\\") == 1)

        local explain = vfs:explain("textures/foo.dds")
        assert(explain.winner.source.path == high)
        assert(explain.winner.sourceIndex == 2 and explain.overridden[1].sourceIndex == 1)
        assert(#explain.overridden == 1)
        assert(#vfs:duplicates().entries == 1 and vfs:duplicates().entries[1].winnerIndex == 2)
        assert(#vfs:duplicates("^textures/").entries == 1)
        assert(#vfs:duplicates("^meshes/").entries == 0)
        assert(#vfs:materializationPlan(high, { allowCopying = true }).actions >= 1)

        local layer = vfs:layerIndex()
        assert(#layer:keys() == 2)
        assert(#layer:providerChain("textures/foo.dds") == 2)
        assert(#layer:sourceContributions().sources == 2)
        assert(layer:sourceContributions().sources[2].sourceIndex == 2)

        local provenance = layer:provenance(vfs, "textures/foo.dds", true)
        assert(provenance.winner.path == high)
        assert(provenance.providers[2].size == 4i)
        local lock = layer:lockManifest(vfs)
        assert(lock:schemaVersion() == 1)
        assert(#lock:entries() == 2)
        assert(#layer:diffAgainstLock(vfs, lock).entries == 0)
        assert(layer:semanticConflicts(vfs, { includeSemanticDeltas = true }).entries[1].allIdentical)

        assert(#conflict:sources() == 2)
        local containing = conflict:sourcesContaining("textures/foo.dds")
        assert(#containing == 2 and containing[1] == 1 and containing[2] == 2)
        assert(#conflict:conflictsReport(true).sources == 2)
        assert(#conflict:shadowedReport(true).sources == 1)
        assert(#conflict:shadowedReport(true, false).sources[1].shadowedFiles == 0)
        assert(#conflict:diffReport(low, high).shared == 1)
        assert(vfs:findByRegex("^textures/"):count() == 1)
        assert(vfs:display():find("foo.dds", 1, true))
    "#,
        )
        .unwrap();
}

#[test]
fn lua_vfs_reveals_lower_provider_and_accepts_manual_provider() {
    let low = TempDir::new("lua_mutable_low");
    let high = TempDir::new("lua_mutable_high");
    let manual = TempDir::new("lua_mutable_manual");
    low.write("shared.txt", b"low");
    high.write("shared.txt", b"high");
    let manual_file = manual.write("manual.txt", b"manual");

    let runtime = runtime_with(&[
        ("low", &low.lua()),
        ("high", &high.lua()),
        ("manualRoot", &manual.lua()),
        ("manualFile", &manual_file.to_string_lossy()),
    ]);
    runtime
        .exec(
            r#"
        local vfs = vfstool.VFS.fromDirectories({ low, high })
        assert(#vfs:providersFor("shared.txt") == 2)
        assert(vfs:providersFor("shared.txt")[1].source.path == low)
        local removed = vfs:removeWinner("shared.txt")
        assert(removed:source().path == high and removed:file():readAll() == "high")
        assert(vfs:getFile("shared.txt"):path():find(low, 1, true) == 1)

        local file = vfstool.VfsFile.from(manualFile)
        local provider = vfstool.VfsProvider.new({ path = manualRoot, kind = "looseDir" }, file)
        assert(vfs:pushProvider("manual.txt", provider))
        assert(vfs:contains("manual.txt"))
        assert(#vfs:removeSource(manualRoot) == 1)
        assert(vfs:contains("manual.txt") == false)
        assert(vfs:setWinnerLooseFile("new/thing.txt", manualFile) == nil)
        assert(vfs:getFile("new/thing.txt"):path() == manualFile)
        assert(#vfs:removeResolvedPrefix("new") == 1 and vfs:len() == 1)
        vfs:pushDirectory(manualRoot)
        assert(vfs:contains("manual.txt"))
        assert(#vfs:removeResolvedMatchingGlob("**/*.txt") == 2 and vfs:isEmpty())
    "#,
        )
        .unwrap();
}

#[test]
fn lua_top_level_helpers_and_run_workflow() {
    let data = TempDir::new("lua_run_data");
    let merged = TempDir::new("lua_run_merged");
    let output = TempDir::new("lua_run_output");
    data.write("config/settings.ini", b"[x]\na = 1\n");

    let runtime = runtime_with(&[
        ("data", &data.lua()),
        ("merged", &merged.lua()),
        ("output", &output.lua()),
    ]);
    // Luau has no `io` library; the script edits merged files through this helper.
    let write_file = runtime
        .bind_function("dream.tests.writeFile", |path: &[u8], text: &[u8]| {
            fs::write(String::from_utf8_lossy(path).as_ref(), text)
                .map_err(|e| l3i::Error::runtime(e.to_string()))
        })
        .unwrap();
    runtime.set_global("writeFile", &write_file).unwrap();
    runtime
        .exec(
            r##"
        assert(vfstool.normalizeHostPath("Textures\\Foo.DDS") == "textures/foo.dds")
        assert(vfstool.normalizeHostPathInPlace("Textures\\Foo.DDS") == "textures/foo.dds")
        assert(vfstool.pathGlobMatches("config/**", "config/settings.ini"))
        assert(vfstool.sourceGlobMatches("**", data))
        local semantic = vfstool.analyzePair("settings.ini", "[x]\na=1\n", "# comment\n[x]\na=1\n")
        assert(semantic.assetClass == "ini")
        assert(semantic.delta.kind == "cosmeticOnly")

        local vfs = vfstool.VFS.fromDirectories({ data })
        local count, snapshot = vfstool.runSetup(vfs, merged, false)
        assert(count == 1 and snapshot:len() == 1)
        writeFile(merged .. "/config/settings.ini", "[x]\na = 2\n")
        assert(#vfstool.changedFiles(merged, snapshot) == 1)
        local copied = vfstool.runFinalize(merged, output, snapshot)
        assert(vfstool.normalizeHostPath(copied[1].relativePath) == "config/settings.ini")

        local _, tracked = vfstool.runSetupTracked(vfs, merged, false)
        writeFile(merged .. "/new.txt", "new")
        assert(#vfstool.changedFilesMetadata(merged, tracked) == 1)
        assert(#vfstool.runFinalizeTracked(merged, output, tracked) == 1)
        assert(vfstool.snapshotDirectory(output):len() == 2)
    "##,
        )
        .unwrap();
}

#[test]
#[cfg(feature = "serialize")]
fn lua_serialize_helper_is_available_with_serialize_feature() {
    let runtime = runtime_with(&[]);
    runtime
        .exec(
            r#"
        local encoded = vfstool.serialize({ answer = 42 }, "json")
        assert(encoded:find("answer", 1, true))
        assert(vfstool.serialize({ 1, 2, 3 }, "json") == "[1,2,3]")
        assert(vfstool.serialize({ a = { b = true } }, "toml"):find("[a]", 1, true))
    "#,
        )
        .unwrap();
}

#[test]
#[cfg(feature = "serialize")]
fn lua_serialize_writes_views_that_hold_files_and_tree_streams() {
    let dir = TempDir::new("lua_serialize_views");
    dir.write("Textures/Rock.dds", b"rock");
    let runtime = runtime_with(&[("dir", &dir.lua())]);
    runtime
        .exec(
            r#"
        local vfs = vfstool.VFS.fromDirectories({ dir })
        local rock = dir .. "/Textures/Rock.dds"
        local file = '{"isArchive":false,"isLoose":true,"path":' .. vfstool.serialize({ rock }, "json"):sub(2, -2) .. '}'

        assert(vfstool.serialize(vfs:entries(), "json")
            == '[{"file":' .. file .. ',"key":"textures/rock.dds"}]')
        local providers = vfstool.serialize(vfs:providersFor("textures/rock.dds"), "json")
        assert(providers:find('"file":' .. file, 1, true), providers)
        assert(providers:find('"kind":"looseDir"', 1, true), providers)
        local tree = vfstool.serialize(vfs:tree(), "json")
        assert(tree:find('"textures":{"files":[' .. file .. '],"subdirs":{}}', 1, true), tree)
        assert(vfstool.serialize({ found = vfs:getFile("textures/rock.dds") }, "yaml"):find("isLoose: true", 1, true))
    "#,
        )
        .unwrap();
}

#[test]
fn lua_vfs_keys_are_sorted() {
    let dir = TempDir::new("lua_vfs_keys_sorted");
    for index in 0..64 {
        dir.write(&format!("dir{}/file{index}.txt", index % 7), b"");
    }

    let runtime = runtime_with(&[("dir", &dir.lua())]);
    runtime
        .exec(
            r"
        local keys = vfstool.VFS.fromDirectories({ dir }):keys()
        assert(#keys == 64)
        for index = 2, #keys do
            assert(keys[index - 1] < keys[index], keys[index - 1] .. ' before ' .. keys[index])
        end
        local plain = keys:toTable()
        assert(#plain == 64)
        table.sort(plain)
        for index = 1, 64 do assert(plain[index] == keys[index]) end
    ",
        )
        .unwrap();
}

#[test]
fn a_host_can_push_its_own_vfs() {
    let dir = TempDir::new("lua_host_vfs");
    dir.write("a.txt", b"a");
    let runtime = runtime_with(&[]);
    let vfs = vfstool_lib::VFS::from_directories([dir.0.as_path()], None);
    {
        let stack = runtime.stack();
        let frame = stack.frame();
        vfstool_lib::lua::Vfs::push(&frame, vfs).unwrap();
        frame.set_global("hostVfs").unwrap();
    }
    runtime
        .exec("assert(hostVfs:len() == 1 and hostVfs:getFile('A.TXT'):readAll() == 'a')")
        .unwrap();
}

#[test]
fn a_host_can_fill_a_keys_view() {
    use vfstool_lib::lua::{KeyBlob, Keys};

    let keys = Keys(KeyBlob::new([&b"a.txt"[..], b"b\xff.txt"]));
    assert_eq!(keys.0.len(), 2);
    assert_eq!(keys.0.get(1), Some(&b"b\xff.txt"[..]));
}
