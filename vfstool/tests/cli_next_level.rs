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
