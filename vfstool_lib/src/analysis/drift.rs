// SPDX-License-Identifier: MIT OR Apache-2.0
use super::{DriftEntry, DriftKind, DriftReport, LayerIndex, VfsLock, VfsLockEntry};
use crate::VFS;
use ahash::AHashMap;
use std::{collections::BTreeMap, io, path::PathBuf};

impl LayerIndex {
    /// Compare current VFS state against a lock manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when building current lock state fails.
    pub fn diff_against_lock(&self, vfs: &VFS, expected: &VfsLock) -> io::Result<DriftReport> {
        let current = self.lock_manifest(vfs)?;

        let mut expected_map: AHashMap<PathBuf, &VfsLockEntry> = AHashMap::new();
        for row in &expected.entries {
            expected_map.insert(row.key.clone(), row);
        }

        let mut current_map: AHashMap<PathBuf, &VfsLockEntry> = AHashMap::new();
        for row in &current.entries {
            current_map.insert(row.key.clone(), row);
        }

        let mut entries = Vec::<DriftEntry>::new();

        for key in current_map.keys() {
            if !expected_map.contains_key(key) {
                entries.push(DriftEntry {
                    key: key.clone(),
                    kind: DriftKind::Added,
                });
            }
        }

        for key in expected_map.keys() {
            if !current_map.contains_key(key) {
                entries.push(DriftEntry {
                    key: key.clone(),
                    kind: DriftKind::Removed,
                });
            }
        }

        for (key, expected_row) in &expected_map {
            let Some(current_row) = current_map.get(key) else {
                continue;
            };

            if expected_row.winner_source != current_row.winner_source {
                entries.push(DriftEntry {
                    key: (*key).clone(),
                    kind: DriftKind::WinnerSourceChanged,
                });
            }

            if expected_row.winner_hash_blake3 != current_row.winner_hash_blake3 {
                entries.push(DriftEntry {
                    key: (*key).clone(),
                    kind: DriftKind::WinnerHashChanged,
                });
            }

            if expected_row.provider_count != current_row.provider_count {
                entries.push(DriftEntry {
                    key: (*key).clone(),
                    kind: DriftKind::ProviderCountChanged,
                });
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
