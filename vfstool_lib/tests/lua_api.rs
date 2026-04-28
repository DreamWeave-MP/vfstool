// SPDX-License-Identifier: GPL-3.0-only

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
        local vfs, conflict = vfstool.VFS.from_directories_with_conflict_index({ low, high })
        assert(vfs:len() == 2)
        assert(vfs:contains("TEXTURES\\FOO.DDS"))
        assert(vfs:get_file("textures/foo.dds"):is_loose())
        assert(vfs:paths_matching("textures")[1].key == "textures/foo.dds")

        local explain = vfs:explain("textures/foo.dds")
        assert(explain.winner.source.path == high)
        assert(#explain.overridden == 1)
        assert(#vfs:duplicates().entries == 1)
        assert(#vfs:case_collisions().collisions == 1)
        assert(#vfs:validate().issues == 1)
        assert(#vfs:materialization_plan(high, { allow_copying = true }).actions >= 1)

        local layer = vfs:layer_index()
        assert(#layer:keys() == 2)
        assert(#layer:provider_chain("textures/foo.dds") == 2)
        assert(#layer:source_contributions().sources == 2)

        local provenance = layer:provenance(vfs, "textures/foo.dds", true)
        assert(provenance.winner.path == high)
        local lock = layer:lock_manifest(vfs)
        assert(lock:schema_version() == 1)
        assert(#lock:entries() == 2)
        assert(#layer:diff_against_lock(vfs, lock).entries == 0)
        assert(layer:semantic_conflicts(vfs, { include_semantic_deltas = true }).entries[1].all_identical)

        assert(#conflict:sources() == 2)
        assert(#conflict:sources_containing("textures/foo.dds") == 2)
        assert(#conflict:conflicts_report(true).sources == 2)
        assert(#conflict:shadowed_report(true).sources == 1)
        assert(#conflict:diff_report(low, high).shared == 1)
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
        .set("manual_root", manual.path().to_string_lossy().as_ref())
        .unwrap();
    lua.globals()
        .set("manual_file", manual_file.to_string_lossy().as_ref())
        .unwrap();
    lua.load(
        r#"
        local vfs = vfstool.VFS.from_directories({ low, high })
        assert(#vfs:providers_for("shared.txt") == 2)
        local removed = vfs:remove_winner("shared.txt")
        assert(removed:source().path == high)
        assert(vfs:get_file("shared.txt"):path():find(low, 1, true) == 1)

        local file = vfstool.VfsFile.from(manual_file)
        local provider = vfstool.VfsProvider.new({ path = manual_root, kind = "loose_dir" }, file)
        assert(vfs:push_provider("manual.txt", provider))
        assert(vfs:contains("manual.txt"))
        assert(#vfs:remove_source(manual_root) == 1)
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
    lua.load(
        r##"
        assert(vfstool.normalize_host_path("Textures\\Foo.DDS") == "textures/foo.dds")
        assert(vfstool.path_glob_matches("config/**", "config/settings.ini"))
        assert(vfstool.source_glob_matches("**", data))
        local semantic = vfstool.analyze_pair("settings.ini", "[x]\na=1\n", "# comment\n[x]\na=1\n")
        assert(semantic.asset_class == "ini")
        assert(semantic.delta.kind == "cosmetic_only")

        local vfs = vfstool.VFS.from_directories({ data })
        local count, snapshot = vfstool.run_setup(vfs, merged, false)
        assert(count == 1)
        local f = io.open(merged .. "/config/settings.ini", "w")
        f:write("[x]\na = 2\n")
        f:close()
        assert(#vfstool.changed_files(merged, snapshot) == 1)
        local copied = vfstool.run_finalize(merged, output, snapshot)
        assert(copied[1].relative_path == "config/settings.ini")

        local _, tracked = vfstool.run_setup_tracked(vfs, merged, false)
        local f2 = io.open(merged .. "/new.txt", "w")
        f2:write("new")
        f2:close()
        assert(#vfstool.changed_files_metadata(merged, tracked) == 1)
        assert(#vfstool.run_finalize_tracked(merged, output, tracked) == 1)
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
