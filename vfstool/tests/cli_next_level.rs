// SPDX-License-Identifier: GPL-3.0-only

use dream_archive::Tes3BsaBuilder;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
use vfstool_lib::serde_json::{self, Value};

struct Fixture {
    root: PathBuf,
    config_dir: PathBuf,
    low: PathBuf,
    high: PathBuf,
    data_local: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("vfstool_cli_{name}_{unique}"));
        let config_dir = root.join("cfg");
        let low = root.join("low");
        let high = root.join("high");
        let data_local = root.join("data_local");

        fs::create_dir_all(&config_dir).expect("config dir should be creatable");
        fs::create_dir_all(&low).expect("low dir should be creatable");
        fs::create_dir_all(&high).expect("high dir should be creatable");
        fs::create_dir_all(&data_local).expect("data-local dir should be creatable");

        write_file(&low.join("scripts/x.lua"), b"print('low')\n");
        write_file(&high.join("scripts/x.lua"), b"print('high')\n");
        write_file(&low.join("config/example.ini"), b"[sec]\na=1\n");
        write_file(&high.join("config/example.ini"), b"[sec]\na=2\n");
        write_file(&low.join("textures/a.dds"), b"low");
        write_file(&high.join("textures/a.dds"), b"high");

        let config = format!(
            "data=\"{}\"\ndata=\"{}\"\ndata-local=\"{}\"\n",
            low.display(),
            high.display(),
            data_local.display()
        );
        fs::write(config_dir.join("openmw.cfg"), config).expect("openmw.cfg should be writable");

        Self {
            root,
            config_dir,
            low,
            high,
            data_local,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(vfstool_bin())
            .arg("--config")
            .arg(&self.config_dir)
            .args(args)
            .output()
            .expect("vfstool command should spawn")
    }

    fn run_with_openmw_config(config_file: &Path, args: &[&str]) -> Output {
        Command::new(vfstool_bin())
            .env("OPENMW_CONFIG", config_file)
            .args(args)
            .output()
            .expect("vfstool command should spawn")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn vfstool_bin() -> &'static str {
    env!("CARGO_BIN_EXE_vfstool")
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, bytes).expect("file should be writable");
}

fn write_text(path: &Path, text: &str) {
    write_file(path, text.as_bytes());
}

fn create_tes3_bsa_archive(archive_dir: &Path, archive_name: &str, paths: &[&str]) -> PathBuf {
    let archive_path = archive_dir.join(archive_name);
    let mut builder = Tes3BsaBuilder::new();
    for path in paths {
        builder
            .add_bytes(*path, path.as_bytes())
            .expect("archive entry should be accepted");
    }
    builder
        .write_path(&archive_path)
        .expect("test archive should be writable");
    archive_path
}

fn quote_path(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be valid json")
}

#[test]
fn openmw_config_env_path_supports_nonstandard_filename_and_quoted_paths() {
    let fixture = Fixture::new("env_config");
    let spaced = fixture.path("data dir with spaces");
    fs::create_dir_all(&spaced).expect("spaced data dir should be creatable");
    write_file(&spaced.join("textures/env.dds"), b"env");

    let custom_config = fixture.path("custom-openmw.cfg");
    let config = format!(
        "data={}\ndata-local={}\n",
        quote_path(&spaced),
        quote_path(&fixture.data_local)
    );
    fs::write(&custom_config, config).expect("custom config should be writable");

    let output = Fixture::run_with_openmw_config(
        &custom_config,
        &["find-file", "textures/env.dds", "--simple"],
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert_eq!(
        stdout.trim(),
        spaced.join("textures/env.dds").display().to_string()
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("json file should be readable"))
        .expect("file should contain valid json")
}

#[test]
fn lock_then_drift_detects_hash_change_and_exits_four() {
    let fixture = Fixture::new("drift");
    let lock_path = fixture.path("lock.json");

    let lock = fixture.run(&[
        "lock",
        "--format",
        "json",
        "--output",
        lock_path.to_str().expect("lock path should be utf-8"),
    ]);
    assert_eq!(lock.status.code(), Some(0));

    write_text(&fixture.high.join("scripts/x.lua"), "print('changed')\n");

    let drift = fixture.run(&[
        "drift",
        lock_path.to_str().expect("lock path should be utf-8"),
        "--fail-on-drift",
        "--format",
        "json",
    ]);
    assert_eq!(drift.status.code(), Some(4));
    let payload = stdout_json(&drift);
    assert!(
        payload["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
    );
}

#[test]
fn bad_regex_exit_code_is_six() {
    let fixture = Fixture::new("bad_regex");
    let output = fixture.run(&["find", "(", "--format", "json"]);

    assert_eq!(output.status.code(), Some(6));
}

#[test]
fn find_file_missing_exits_one() {
    let fixture = Fixture::new("find_missing");
    let output = fixture.run(&["find-file", "meshes/missing.nif", "--simple"]);

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn explain_missing_path_exits_one() {
    let fixture = Fixture::new("explain_missing");
    let output = fixture.run(&["explain", "meshes/missing.nif", "--format", "json"]);

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn malformed_openmw_config_exits_seven() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("vfstool_bad_cfg_{unique}"));
    let config_dir = root.join("cfg");
    fs::create_dir_all(&config_dir).expect("config dir should be creatable");
    write_text(
        &config_dir.join("openmw.cfg"),
        "this is definitely not valid openmw cfg !!!\n",
    );

    let output = Command::new(vfstool_bin())
        .arg("--config")
        .arg(&config_dir)
        .arg("contributions")
        .output()
        .expect("vfstool command should spawn");
    let _ = fs::remove_dir_all(&root);

    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn explain_reports_winner_and_overridden_providers() {
    let fixture = Fixture::new("explain");
    let output = fixture.run(&["explain", "textures/a.dds", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    assert_eq!(
        payload["winner"]["source"]["path"],
        fixture.high.display().to_string()
    );
    assert_eq!(payload["overridden"].as_array().map(Vec::len), Some(1));
}

#[test]
fn duplicate_report_lists_shared_vfs_keys() {
    let fixture = Fixture::new("duplicates");
    let output = fixture.run(&["duplicates", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let entries = payload["entries"]
        .as_array()
        .expect("entries should be an array");
    assert!(entries.iter().any(|entry| entry["key"] == "textures/a.dds"));
}

#[test]
fn duplicate_report_can_filter_by_vfs_key_regex() {
    let fixture = Fixture::new("duplicates_filter");
    write_file(&fixture.low.join("meshes/a.nif"), b"low mesh");
    write_file(&fixture.high.join("meshes/a.nif"), b"high mesh");
    let output = fixture.run(&["duplicates", "^meshes/", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let entries = payload["entries"]
        .as_array()
        .expect("entries should be an array");
    assert!(entries.iter().any(|entry| entry["key"] == "meshes/a.nif"));
    assert!(!entries.iter().any(|entry| entry["key"] == "textures/a.dds"));
}

#[test]
fn duplicate_report_bad_regex_exits_six() {
    let fixture = Fixture::new("duplicates_bad_regex");
    let output = fixture.run(&["duplicates", "[invalid", "--format", "json"]);

    assert_eq!(output.status.code(), Some(6));
}

#[test]
fn archive_list_accepts_unique_archive_filename() {
    let fixture = Fixture::new("archive_filename");
    create_tes3_bsa_archive(&fixture.low, "Morrowind.bsa", &["Meshes/Used.NIF"]);
    write_text(
        &fixture.config_dir.join("openmw.cfg"),
        &format!(
            "data=\"{}\"\ndata=\"{}\"\ndata-local=\"{}\"\nfallback-archive=Morrowind.bsa\n",
            fixture.low.display(),
            fixture.high.display(),
            fixture.data_local.display()
        ),
    );

    let output = fixture.run(&["archive-list", "Morrowind.bsa", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let entries = payload.as_array().expect("archive-list should be an array");
    assert!(entries.iter().any(|entry| {
        entry["key"] == "meshes/used.nif"
            && entry["archive_path"] == fixture.low.join("Morrowind.bsa").display().to_string()
            && entry["wins"] == true
    }));
}

#[test]
fn archive_list_accepts_source_index_from_archives_report() {
    let fixture = Fixture::new("archive_source_index");
    create_tes3_bsa_archive(&fixture.low, "Morrowind.bsa", &["Meshes/Low.NIF"]);
    create_tes3_bsa_archive(&fixture.high, "Tribunal.bsa", &["Meshes/High.NIF"]);
    write_text(
        &fixture.config_dir.join("openmw.cfg"),
        &format!(
            "data=\"{}\"\ndata=\"{}\"\ndata-local=\"{}\"\nfallback-archive=Morrowind.bsa\nfallback-archive=Tribunal.bsa\n",
            fixture.low.display(),
            fixture.high.display(),
            fixture.data_local.display()
        ),
    );

    let archives = fixture.run(&["archives", "--format", "json"]);
    assert_eq!(archives.status.code(), Some(0));
    let archives_payload = stdout_json(&archives);
    let tribunal = archives_payload
        .as_array()
        .expect("archives should be an array")
        .iter()
        .find(|entry| {
            entry["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("Tribunal.bsa"))
        })
        .expect("Tribunal.bsa should be loaded");
    let source_index = tribunal["source_index"]
        .as_u64()
        .expect("source index should be numeric")
        .to_string();

    let output = fixture.run(&[
        "archive-list",
        "--source-index",
        &source_index,
        "--format",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let entries = payload.as_array().expect("archive-list should be an array");
    assert!(
        entries
            .iter()
            .any(|entry| entry["key"] == "meshes/high.nif")
    );
}

#[test]
fn output_flag_writes_json_file_without_stdout_payload() {
    let fixture = Fixture::new("output_flag");
    let output_path = fixture.path("duplicates.json");
    let output = fixture.run(&[
        "duplicates",
        "--format",
        "json",
        "--output",
        output_path.to_str().expect("output path should be utf-8"),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let payload = read_json(&output_path);
    assert!(
        payload["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
    );
}

#[test]
fn validate_reports_file_directory_conflict_json() {
    let fixture = Fixture::new("validate_conflict");
    write_file(&fixture.low.join("blocked"), b"file");
    write_file(&fixture.high.join("blocked/child.txt"), b"child");

    let output = fixture.run(&["validate", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let issues = payload["issues"]
        .as_array()
        .expect("issues should be array");
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("FileDirectoryConflict").is_some())
    );
}

#[test]
fn validate_reports_missing_openmw_config_sources() {
    let fixture = Fixture::new("validate_config_sources");
    let missing_data = fixture.path("missing-data");
    write_text(
        &fixture.config_dir.join("openmw.cfg"),
        &format!(
            "data=\"{}\"\ndata=\"{}\"\ndata-local=\"{}\"\nfallback-archive=Missing.bsa\ncontent=Missing.esm\ngroundcover=MissingGrass.esp\n",
            fixture.low.display(),
            missing_data.display(),
            fixture.data_local.display()
        ),
    );

    let output = fixture.run(&["validate", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let issues = payload["issues"]
        .as_array()
        .expect("issues should be array");
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("MissingDataDirectory").is_some())
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("MissingFallbackArchive").is_some())
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("MissingContentFile").is_some())
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("MissingGroundcoverFile").is_some())
    );
    assert!(
        !issues
            .iter()
            .any(|issue| issue.get("CaseCollision").is_some())
    );
}

#[test]
fn shadowed_omits_file_lists_unless_requested() {
    let fixture = Fixture::new("shadowed_summary");

    let summary = fixture.run(&["shadowed", "--format", "json"]);
    assert_eq!(summary.status.code(), Some(0));
    let summary_payload = stdout_json(&summary);
    let sources = summary_payload["sources"]
        .as_array()
        .expect("sources should be an array");
    let low = sources
        .iter()
        .find(|source| source["path"] == fixture.low.display().to_string())
        .expect("low source should be fully shadowed");
    assert!(low.get("shadowed_files").is_none());

    let detailed = fixture.run(&["shadowed", "--list-files", "--format", "json"]);
    assert_eq!(detailed.status.code(), Some(0));
    let detailed_payload = stdout_json(&detailed);
    let detailed_sources = detailed_payload["sources"]
        .as_array()
        .expect("sources should be an array");
    let low_detailed = detailed_sources
        .iter()
        .find(|source| source["path"] == fixture.low.display().to_string())
        .expect("low source should be fully shadowed");
    assert!(
        low_detailed["shadowed_files"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
    );
}

#[test]
fn diff_between_sources_reports_shared_and_unique_keys() {
    let fixture = Fixture::new("diff");
    write_file(&fixture.low.join("only-low.txt"), b"low");
    write_file(&fixture.high.join("only-high.txt"), b"high");

    let output = fixture.run(&[
        "diff",
        fixture.low.to_str().expect("low path should be utf-8"),
        fixture.high.to_str().expect("high path should be utf-8"),
        "--format",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    assert!(
        payload["shared"]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key == "textures/a.dds")
    );
    assert!(
        payload["only_in_a"]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key == "only-low.txt")
    );
    assert!(
        payload["only_in_b"]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key == "only-high.txt")
    );
}

#[test]
fn collapse_dry_run_prints_plan_without_writing_target() {
    let fixture = Fixture::new("collapse_dry_run");
    let target = fixture.path("merged");
    let output = fixture.run(&[
        "collapse",
        target.to_str().expect("target path should be utf-8"),
        "--dry-run",
        "--allow-copying",
        "--format",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    assert!(
        payload["actions"]
            .as_array()
            .is_some_and(|actions| !actions.is_empty())
    );
    assert!(!target.exists());
}

#[test]
#[cfg(unix)]
fn run_captures_new_file_to_data_local_and_removes_merged_dir() {
    let fixture = Fixture::new("run_capture");
    let merged = fixture.path("merged");

    let output = fixture.run(&[
        "run",
        "--copy",
        merged.to_str().expect("merged path should be utf-8"),
        "--",
        "sh",
        "-c",
        "printf captured > \"$1/generated.txt\"",
        "sh",
        "{}",
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(!merged.exists());
    assert_eq!(
        fs::read_to_string(fixture.data_local.join("generated.txt"))
            .expect("captured file should exist")
            .trim(),
        "captured"
    );
}
