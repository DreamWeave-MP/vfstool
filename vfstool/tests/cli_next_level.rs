// SPDX-License-Identifier: MIT OR Apache-2.0

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    config_dir: PathBuf,
    low: PathBuf,
    high: PathBuf,
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

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be valid json")
}

#[test]
fn solve_satisfiable_writes_order_file() {
    let fixture = Fixture::new("solve_sat");
    let constraints = fixture.path("constraints_sat.yaml");
    let solved_order = fixture.path("solved_order.txt");
    write_text(&constraints, "constraints: []\n");

    let output = fixture.run(&[
        "solve",
        constraints
            .to_str()
            .expect("constraints path should be utf-8"),
        "--objective",
        "min_moves",
        "--write-order",
        solved_order.to_str().expect("order path should be utf-8"),
        "--format",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    assert_eq!(payload["status"], "Satisfiable");
    let order_lines = fs::read_to_string(&solved_order).expect("order file should be readable");
    assert!(order_lines.lines().count() >= 2);
}

#[test]
fn solve_unsat_exits_with_code_five() {
    let fixture = Fixture::new("solve_unsat");
    let constraints = fixture.path("constraints_unsat.yaml");
    write_text(
        &constraints,
        "constraints:\n  - type: winner_must_be\n    path_glob: \"scripts/x.lua\"\n    source_glob: \"**/low\"\n  - type: winner_must_be\n    path_glob: \"scripts/x.lua\"\n    source_glob: \"**/high\"\n",
    );

    let output = fixture.run(&[
        "solve",
        constraints
            .to_str()
            .expect("constraints path should be utf-8"),
        "--format",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(5));
    let payload = stdout_json(&output);
    assert_eq!(payload["status"], "Unsatisfiable");
    assert!(
        payload["diagnostics"]["violated_constraints"]
            .as_array()
            .is_some_and(|violations| !violations.is_empty())
    );
}

#[test]
fn semantic_conflicts_enrich_reports_asset_metadata() {
    let fixture = Fixture::new("semantic_enrich");
    let output = fixture.run(&["semantic-conflicts", "--enrich", "--format", "json"]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    let entries = payload["entries"]
        .as_array()
        .expect("entries should be an array");
    assert!(!entries.is_empty());

    let has_enriched = entries.iter().any(|entry| {
        entry["asset_class"] != Value::String("Unknown".to_owned())
            && entry["providers"].as_array().is_some_and(|providers| {
                providers
                    .iter()
                    .any(|p| !p["semantic_delta_to_winner"].is_null())
            })
    });
    assert!(has_enriched);
}

#[test]
fn simulate_swap_with_impact_profile_includes_impact_report() {
    let fixture = Fixture::new("simulate_impact");
    let impact = fixture.path("impact.yaml");
    write_text(
        &impact,
        "heuristics:\n  - name: scripts\n    path_glob: \"scripts/**\"\n    weight: 2.0\n    condition: winner_changed\n",
    );

    let output = fixture.run(&[
        "simulate-swap",
        fixture.low.to_str().expect("low path should be utf-8"),
        fixture.high.to_str().expect("high path should be utf-8"),
        "--impact-profile",
        impact.to_str().expect("impact path should be utf-8"),
        "--format",
        "json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let payload = stdout_json(&output);
    assert!(payload["simulation"].is_object());
    assert!(payload["impact"].is_object());
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
fn verify_policy_violations_exit_three() {
    let fixture = Fixture::new("verify_violation");
    let policy = fixture.path("policy.yaml");
    write_text(
        &policy,
        "rules:\n  - type: winner_must_match\n    path_glob: \"scripts/**\"\n    source_glob: \"**/low\"\n",
    );

    let output = fixture.run(&[
        "verify",
        policy.to_str().expect("policy path should be utf-8"),
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(3));

    let payload = stdout_json(&output);
    assert!(
        payload["violations"]
            .as_array()
            .is_some_and(|violations| !violations.is_empty())
    );
}

#[test]
fn kb_add_and_lookup_roundtrip_finds_match() {
    let fixture = Fixture::new("kb_roundtrip");
    let report_path = fixture.path("semantic.json");
    let store_path = fixture.path("knowledge.tsv");

    let semantic = fixture.run(&[
        "semantic-conflicts",
        "--enrich",
        "--format",
        "json",
        "--output",
        report_path.to_str().expect("report path should be utf-8"),
    ]);
    assert_eq!(semantic.status.code(), Some(0));

    let report: Value = serde_json::from_str(
        &fs::read_to_string(&report_path).expect("semantic report should be readable"),
    )
    .expect("semantic report should parse as json");
    let entry = report["entries"]
        .as_array()
        .and_then(|entries| entries.first())
        .expect("semantic report should contain entries");
    let winner_path = entry["winner"]["path"]
        .as_str()
        .expect("winner path should be present");
    let key = entry["key"].as_str().expect("key should be present");
    let (loser_path, low_hash) = entry["providers"]
        .as_array()
        .and_then(|providers| {
            providers.iter().find_map(|provider| {
                let source = provider["source"]["path"].as_str()?;
                if source == winner_path {
                    None
                } else {
                    Some((source, provider["hash_blake3"].as_str().unwrap_or_default()))
                }
            })
        })
        .expect("a non-winner provider should exist");
    let high_hash = entry["providers"]
        .as_array()
        .and_then(|providers| {
            providers.iter().find_map(|provider| {
                let source = provider["source"]["path"].as_str()?;
                if source == winner_path {
                    provider["hash_blake3"].as_str()
                } else {
                    None
                }
            })
        })
        .unwrap_or_default();

    let add = fixture.run(&[
        "kb-add",
        store_path.to_str().expect("store path should be utf-8"),
        loser_path,
        winner_path,
        key,
        "--outcome",
        "safe_intentional_override",
        "--confidence",
        "0.8",
        "--notes",
        "integration test",
        "--low-hash",
        low_hash,
        "--high-hash",
        high_hash,
        "--format",
        "json",
    ]);
    assert_eq!(add.status.code(), Some(0));

    let lookup = fixture.run(&[
        "kb-lookup",
        store_path.to_str().expect("store path should be utf-8"),
        report_path.to_str().expect("report path should be utf-8"),
        "--format",
        "json",
    ]);
    assert_eq!(lookup.status.code(), Some(0));
    let payload = stdout_json(&lookup);
    assert!(
        payload["matches"]
            .as_array()
            .is_some_and(|matches| !matches.is_empty())
    );
}

#[test]
fn solve_output_is_deterministic_across_runs() {
    let fixture = Fixture::new("solve_determinism");
    let constraints = fixture.path("constraints.yaml");
    write_text(&constraints, "constraints: []\n");

    let first = fixture.run(&[
        "solve",
        constraints
            .to_str()
            .expect("constraints path should be utf-8"),
        "--format",
        "json",
    ]);
    let second = fixture.run(&[
        "solve",
        constraints
            .to_str()
            .expect("constraints path should be utf-8"),
        "--format",
        "json",
    ]);

    assert_eq!(first.status.code(), Some(0));
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn bad_regex_exit_code_is_six() {
    let fixture = Fixture::new("bad_regex");
    let output = fixture.run(&["find", "(", "--format", "json"]);

    assert_eq!(output.status.code(), Some(6));
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
        .arg("stats")
        .output()
        .expect("vfstool command should spawn");
    let _ = fs::remove_dir_all(&root);

    assert_eq!(output.status.code(), Some(7));
}
