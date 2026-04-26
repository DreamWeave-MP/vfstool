// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::analysis::SemanticConflictReport;
use std::{io, path::PathBuf};

/// Stable conflict fingerprint key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct ConflictFingerprint {
    /// Lower-priority source path.
    pub low_source: PathBuf,
    /// Higher-priority source path.
    pub high_source: PathBuf,
    /// Key pattern or exact key text.
    pub key_pattern: String,
    /// Optional lower provider hash.
    pub low_hash: Option<String>,
    /// Optional higher provider hash.
    pub high_hash: Option<String>,
}

/// Known outcome class for a fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serialize", serde(rename_all = "snake_case"))]
pub enum KnownOutcome {
    /// Known harmless conflict.
    SafeNoOp,
    /// Known override is intentional and acceptable.
    SafeIntentionalOverride,
    /// Human patching is required.
    RequiresManualPatch,
    /// Known breakage pattern.
    KnownBreakage,
}

/// One persisted knowledge entry.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct KnowledgeEntry {
    /// Conflict fingerprint key.
    pub fingerprint: ConflictFingerprint,
    /// Outcome class.
    pub outcome: KnownOutcome,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f32,
    /// Optional notes.
    pub notes: String,
}

/// Knowledge lookup and persistence interface.
pub trait KnowledgeStore {
    /// Return entries matching `fingerprint`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying store cannot be read.
    fn lookup(&self, fingerprint: &ConflictFingerprint) -> io::Result<Vec<KnowledgeEntry>>;
    /// Insert or replace one entry by fingerprint key.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying store cannot be written.
    fn upsert(&mut self, entry: KnowledgeEntry) -> io::Result<()>;
    /// Return all entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying store cannot be read.
    fn all(&self) -> io::Result<Vec<KnowledgeEntry>>;
}

/// File-backed local knowledge store.
///
/// The on-disk format is YAML when the `serialize` feature is enabled. This
/// module is hidden experimental API, so the schema is not promoted as stable.
pub struct LocalKnowledgeStore {
    path: PathBuf,
}

impl LocalKnowledgeStore {
    /// Create a local store at `path`.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load_entries(&self) -> io::Result<Vec<KnowledgeEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        load_entries_from_yaml(&self.path)
    }

    fn save_entries(&self, entries: &[KnowledgeEntry]) -> io::Result<()> {
        save_entries_to_yaml(&self.path, entries)
    }
}

#[cfg(feature = "serialize")]
fn load_entries_from_yaml(path: &std::path::Path) -> io::Result<Vec<KnowledgeEntry>> {
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_yaml::from_str(&content).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid YAML knowledge store '{}': {e}", path.display()),
        )
    })
}

#[cfg(not(feature = "serialize"))]
fn load_entries_from_yaml(path: &std::path::Path) -> io::Result<Vec<KnowledgeEntry>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "YAML knowledge store '{}' requires the serialize feature",
            path.display()
        ),
    ))
}

#[cfg(feature = "serialize")]
fn save_entries_to_yaml(path: &std::path::Path, entries: &[KnowledgeEntry]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(entries)
        .map_err(|e| io::Error::other(format!("failed to serialize knowledge store: {e}")))?;
    std::fs::write(path, text)
}

#[cfg(not(feature = "serialize"))]
fn save_entries_to_yaml(path: &std::path::Path, _entries: &[KnowledgeEntry]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "YAML knowledge store '{}' requires the serialize feature",
            path.display()
        ),
    ))
}

impl KnowledgeStore for LocalKnowledgeStore {
    fn lookup(&self, fingerprint: &ConflictFingerprint) -> io::Result<Vec<KnowledgeEntry>> {
        Ok(self
            .load_entries()?
            .into_iter()
            .filter(|entry| entry.fingerprint == *fingerprint)
            .collect())
    }

    fn upsert(&mut self, entry: KnowledgeEntry) -> io::Result<()> {
        let mut entries = self.load_entries()?;
        if let Some(idx) = entries
            .iter()
            .position(|existing| existing.fingerprint == entry.fingerprint)
        {
            entries[idx] = entry;
        } else {
            entries.push(entry);
        }
        entries.sort_by(|a, b| {
            a.fingerprint
                .low_source
                .cmp(&b.fingerprint.low_source)
                .then(a.fingerprint.high_source.cmp(&b.fingerprint.high_source))
                .then(a.fingerprint.key_pattern.cmp(&b.fingerprint.key_pattern))
        });
        self.save_entries(&entries)
    }

    fn all(&self) -> io::Result<Vec<KnowledgeEntry>> {
        self.load_entries()
    }
}

/// Derive conflict fingerprints from semantic conflict output.
#[must_use]
pub fn conflict_fingerprints_from_report(
    report: &SemanticConflictReport,
) -> Vec<ConflictFingerprint> {
    let mut out = Vec::new();
    for entry in &report.entries {
        if let Some(winner) = entry.providers.last() {
            for provider in entry
                .providers
                .iter()
                .take(entry.providers.len().saturating_sub(1))
            {
                out.push(ConflictFingerprint {
                    low_source: provider.source.path.clone(),
                    high_source: winner.source.path.clone(),
                    key_pattern: entry.key.display().to_string(),
                    low_hash: provider.hash_blake3.clone(),
                    high_hash: winner.hash_blake3.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        SourceKind,
        analysis::{SemanticConflict, SemanticProvider, SemanticRelation, SourceMeta},
    };

    #[cfg(feature = "serialize")]
    #[test]
    fn upsert_replaces_matching_fingerprint() {
        let path = std::env::temp_dir().join("kb_upsert_replaces.yaml");
        let _ = std::fs::remove_file(&path);
        let mut store = LocalKnowledgeStore::new(path.clone());

        let fp = ConflictFingerprint {
            low_source: PathBuf::from("/a"),
            high_source: PathBuf::from("/b"),
            key_pattern: "x.dds".into(),
            low_hash: None,
            high_hash: None,
        };
        store
            .upsert(KnowledgeEntry {
                fingerprint: fp.clone(),
                outcome: KnownOutcome::SafeNoOp,
                confidence: 0.5,
                notes: "a".into(),
            })
            .expect("first upsert should succeed");
        store
            .upsert(KnowledgeEntry {
                fingerprint: fp,
                outcome: KnownOutcome::KnownBreakage,
                confidence: 1.0,
                notes: "b".into(),
            })
            .expect("second upsert should succeed");

        let all = store.all().expect("load all should succeed");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].outcome, KnownOutcome::KnownBreakage);
        let yaml = std::fs::read_to_string(&path).expect("knowledge store should be readable");
        assert!(yaml.contains("fingerprint:"));
        assert!(yaml.contains("known_breakage"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fingerprint_generation_pairs_loser_with_winner() {
        let _layer = crate::LayerIndex::from_file_lists(vec![
            (
                SourceMeta {
                    path: PathBuf::from("/a"),
                    kind: SourceKind::LooseDir,
                },
                vec![PathBuf::from("textures/a.dds")],
            ),
            (
                SourceMeta {
                    path: PathBuf::from("/b"),
                    kind: SourceKind::LooseDir,
                },
                vec![PathBuf::from("textures/a.dds")],
            ),
        ]);

        let report = SemanticConflictReport {
            entries: vec![SemanticConflict {
                key: PathBuf::from("textures/a.dds"),
                winner: SourceMeta {
                    path: PathBuf::from("/b"),
                    kind: SourceKind::LooseDir,
                },
                providers: vec![
                    SemanticProvider {
                        source: SourceMeta {
                            path: PathBuf::from("/a"),
                            kind: SourceKind::LooseDir,
                        },
                        relation: SemanticRelation::DifferentFromWinner,
                        hash_blake3: Some("aa".into()),
                        size: Some(1),
                        semantic_delta_to_winner: None,
                    },
                    SemanticProvider {
                        source: SourceMeta {
                            path: PathBuf::from("/b"),
                            kind: SourceKind::LooseDir,
                        },
                        relation: SemanticRelation::IdenticalToWinner,
                        hash_blake3: Some("bb".into()),
                        size: Some(1),
                        semantic_delta_to_winner: None,
                    },
                ],
                asset_class: crate::semantic::AssetClass::Binary,
                all_identical: false,
                distinct_versions: 2,
            }],
        };

        let fps = conflict_fingerprints_from_report(&report);
        assert_eq!(fps.len(), 1);
        assert_eq!(fps[0].low_source, PathBuf::from("/a"));
        assert_eq!(fps[0].high_source, PathBuf::from("/b"));
    }
}
