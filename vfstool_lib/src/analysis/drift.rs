// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{
    DriftEntry, DriftKind, DriftReport, LayerIndex, VFS_LOCK_SCHEMA_VERSION, VfsLock, VfsLockEntry,
};
use crate::VFS;
use ahash::AHashMap;
use std::{borrow::Cow, collections::BTreeMap, io};

impl LayerIndex {
    /// Compare current VFS state against a lock manifest.
    ///
    /// Keys and winner sources are compared as text, the way a lock file holds them: a key that is
    /// not UTF-8 is written with U+FFFD in place of its invalid bytes, so a lock read back from a
    /// file matches the VFS it was written from. Each entry carries the current key when the VFS
    /// has it, and the lock's key otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error when building current lock state fails.
    pub fn diff_against_lock(&self, vfs: &VFS, expected: &VfsLock) -> io::Result<DriftReport> {
        if expected.schema_version != VFS_LOCK_SCHEMA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported VFS lock schema_version {}; expected {}",
                    expected.schema_version, VFS_LOCK_SCHEMA_VERSION
                ),
            ));
        }

        let current = self.lock_manifest(vfs)?;

        let expected_map: AHashMap<Cow<'_, str>, &VfsLockEntry> = expected
            .entries
            .iter()
            .map(|row| (row.key.to_string_lossy(), row))
            .collect();
        let current_map: AHashMap<Cow<'_, str>, &VfsLockEntry> = current
            .entries
            .iter()
            .map(|row| (row.key.to_string_lossy(), row))
            .collect();

        let mut entries = Vec::<DriftEntry>::new();

        for (key, current_row) in &current_map {
            if !expected_map.contains_key(key) {
                entries.push(DriftEntry {
                    key: current_row.key.clone(),
                    kind: DriftKind::Added,
                });
            }
        }

        for (key, expected_row) in &expected_map {
            if !current_map.contains_key(key) {
                entries.push(DriftEntry {
                    key: expected_row.key.clone(),
                    kind: DriftKind::Removed,
                });
            }
        }

        for (key, expected_row) in &expected_map {
            let Some(current_row) = current_map.get(key) else {
                continue;
            };
            let drifted = |kind| DriftEntry {
                key: current_row.key.clone(),
                kind,
            };

            if expected_row.winner_source.to_string_lossy()
                != current_row.winner_source.to_string_lossy()
            {
                entries.push(drifted(DriftKind::WinnerSourceChanged));
            }

            if expected_row.winner_hash_blake3 != current_row.winner_hash_blake3 {
                entries.push(drifted(DriftKind::WinnerHashChanged));
            }

            if expected_row.provider_count != current_row.provider_count {
                entries.push(drifted(DriftKind::ProviderCountChanged));
            }
        }

        entries.sort_by(|a, b| a.key.cmp(&b.key).then(a.kind.cmp(&b.kind)));
        let mut counts: BTreeMap<DriftKind, usize> = BTreeMap::new();
        for entry in &entries {
            *counts.entry(entry.kind).or_insert(0) += 1;
        }

        Ok(DriftReport { entries, counts })
    }
}
