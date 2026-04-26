use super::*;
use crate::VFS;
use std::fs;
use std::path::{Path, PathBuf};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, data: &[u8]) {
        let target = self.0.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dir");
        }
        fs::write(target, data).expect("failed to write test file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[path = "tests/candidate.rs"]
mod candidate;
#[path = "tests/layer_provenance.rs"]
mod layer_provenance;
#[path = "tests/lock_drift.rs"]
mod lock_drift;
#[path = "tests/semantic.rs"]
mod semantic;
#[path = "tests/simulate_impact.rs"]
mod simulate_impact;
