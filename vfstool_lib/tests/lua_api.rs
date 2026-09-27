// SPDX-License-Identifier: MIT OR Apache-2.0

#![cfg(feature = "lua")]

use std::{fs, path::PathBuf};

use mlua::Lua;

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

    fn path(&self) -> &PathBuf {
        &self.0
    }

    fn write(&self, rel: &str, data: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn lua_with_vfstool() -> Lua {
    let lua = Lua::new();
    vfstool_lib::lua::register(&lua).unwrap();
    lua
}

#[test]
fn lua_vfs_provider_reports_and_layer_workflows() {
    let low = TempDir::new("lua_vfs_low");
    let high = TempDir::new("lua_vfs_high");
    low.write("Textures/Foo.DDS", b"same");
    high.write("textures/foo.dds", b"same");
    high.write("meshes/bar.nif", b"mesh");

    let lua = lua_with_vfstool();
    lua.globals()
        .set("low", low.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("high", high.path().to_string_lossy().as_ref())
        .unwrap();
    lua.load(
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
        assert(#explain.overridden == 1)
        assert(#vfs:duplicates().entries == 1)
        assert(#vfs:duplicates("^textures/").entries == 1)
        assert(#vfs:duplicates("^meshes/").entries == 0)
        assert(#vfs:materializationPlan(high, { allowCopying = true }).actions >= 1)

        local layer = vfs:layerIndex()
        assert(#layer:keys() == 2)
        assert(#layer:providerChain("textures/foo.dds") == 2)
        assert(#layer:sourceContributions().sources == 2)

        local provenance = layer:provenance(vfs, "textures/foo.dds", true)
        assert(provenance.winner.path == high)
        local lock = layer:lockManifest(vfs)
        assert(lock:schemaVersion() == 1)
        assert(#lock:entries() == 2)
        assert(#layer:diffAgainstLock(vfs, lock).entries == 0)
        assert(layer:semanticConflicts(vfs, { includeSemanticDeltas = true }).entries[1].allIdentical)

        assert(#conflict:sources() == 2)
        assert(#conflict:sourcesContaining("textures/foo.dds") == 2)
        assert(#conflict:conflictsReport(true).sources == 2)
        assert(#conflict:shadowedReport(true).sources == 1)
        assert(#conflict:shadowedReport(true, false).sources[1].shadowedFiles == 0)
        assert(#conflict:diffReport(low, high).shared == 1)
    "#,
    )
    .exec()
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

    let lua = lua_with_vfstool();
    lua.globals()
        .set("low", low.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("high", high.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("manualRoot", manual.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("manualFile", manual_file.to_string_lossy().as_ref())
        .unwrap();
    lua.load(
        r#"
        local vfs = vfstool.VFS.fromDirectories({ low, high })
        assert(#vfs:providersFor("shared.txt") == 2)
        local removed = vfs:removeWinner("shared.txt")
        assert(removed:source().path == high)
        assert(vfs:getFile("shared.txt"):path():find(low, 1, true) == 1)

        local file = vfstool.VfsFile.from(manualFile)
        local provider = vfstool.VfsProvider.new({ path = manualRoot, kind = "looseDir" }, file)
        assert(vfs:pushProvider("manual.txt", provider))
        assert(vfs:contains("manual.txt"))
        assert(#vfs:removeSource(manualRoot) == 1)
        assert(vfs:contains("manual.txt") == false)
    "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn lua_top_level_helpers_and_run_workflow() {
    let data = TempDir::new("lua_run_data");
    let merged = TempDir::new("lua_run_merged");
    let output = TempDir::new("lua_run_output");
    data.write("config/settings.ini", b"[x]\na = 1\n");

    let lua = lua_with_vfstool();
    lua.globals()
        .set("data", data.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("merged", merged.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("output", output.path().to_string_lossy().as_ref())
        .unwrap();
    // Luau has no `io` library; the script edits merged files through this helper.
    let write_file = lua
        .create_function(|_, (path, text): (String, String)| {
            fs::write(path, text).map_err(mlua::Error::external)
        })
        .unwrap();
    lua.globals().set("writeFile", write_file).unwrap();
    lua.load(
        r##"
        assert(vfstool.normalizeHostPath("Textures\\Foo.DDS") == "textures/foo.dds")
        assert(vfstool.pathGlobMatches("config/**", "config/settings.ini"))
        assert(vfstool.sourceGlobMatches("**", data))
        local semantic = vfstool.analyzePair("settings.ini", "[x]\na=1\n", "# comment\n[x]\na=1\n")
        assert(semantic.assetClass == "ini")
        assert(semantic.delta.kind == "cosmeticOnly")

        local vfs = vfstool.VFS.fromDirectories({ data })
        local count, snapshot = vfstool.runSetup(vfs, merged, false)
        assert(count == 1)
        writeFile(merged .. "/config/settings.ini", "[x]\na = 2\n")
        assert(#vfstool.changedFiles(merged, snapshot) == 1)
        local copied = vfstool.runFinalize(merged, output, snapshot)
        assert(vfstool.normalizeHostPath(copied[1].relativePath) == "config/settings.ini")

        local _, tracked = vfstool.runSetupTracked(vfs, merged, false)
        writeFile(merged .. "/new.txt", "new")
        assert(#vfstool.changedFilesMetadata(merged, tracked) == 1)
        assert(#vfstool.runFinalizeTracked(merged, output, tracked) == 1)
    "##,
    )
    .exec()
    .unwrap();
}

#[test]
#[cfg(feature = "serialize")]
fn lua_serialize_helper_is_available_with_serialize_feature() {
    let lua = lua_with_vfstool();
    lua.load(
        r#"
        local encoded = vfstool.serialize({ answer = 42 }, "json")
        assert(encoded:find("answer", 1, true))
    "#,
    )
    .exec()
    .unwrap();
}

#[test]
fn lua_vfs_keys_are_sorted() {
    let dir = TempDir::new("lua_vfs_keys_sorted");
    for index in 0..64 {
        dir.write(&format!("dir{}/file{index}.txt", index % 7), b"");
    }

    let lua = lua_with_vfstool();
    lua.globals()
        .set("dir", dir.path().to_string_lossy().as_ref())
        .unwrap();
    lua.load(
        r"
        local keys = vfstool.VFS.fromDirectories({ dir }):keys()
        assert(#keys == 64)
        for index = 2, #keys do
            assert(keys[index - 1] < keys[index], keys[index - 1] .. ' before ' .. keys[index])
        end
    ",
    )
    .exec()
    .unwrap();
}
