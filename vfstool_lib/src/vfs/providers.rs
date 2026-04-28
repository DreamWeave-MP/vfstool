// SPDX-License-Identifier: GPL-3.0-only
use super::VFS;
use crate::{
    CollapseOptions, NormalizedPath, SourceContributionReport, SourceKind, SourceMeta,
    normalize_host_path,
    paths::{key_is_at_or_under_prefix, key_to_path_buf_lossy, key_to_string_lossy},
};
use ahash::{AHashMap, AHashSet};
use std::path::{Path, PathBuf};

/// One source that can provide a normalized VFS key.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct VfsProviderRecord {
    /// Source index in low-to-high priority order.
    pub source_index: usize,
    /// Source metadata for the provider.
    pub source: SourceMeta,
    /// Normalized VFS key provided by this source.
    pub key: PathBuf,
    /// Original loose path or in-archive entry path.
    pub original_path: PathBuf,
    /// Human-readable resolved path, including archive parent when applicable.
    pub resolved_path: String,
}

/// Full provider explanation for a single VFS key.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ExplainReport {
    /// Normalized key that was explained.
    pub key: PathBuf,
    /// Winning provider.
    pub winner: VfsProviderRecord,
    /// Lower-priority providers overridden by the winner.
    pub overridden: Vec<VfsProviderRecord>,
}

/// One duplicate VFS key and all providers for it.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct DuplicateEntry {
    /// Normalized key with more than one provider.
    pub key: PathBuf,
    /// Providers in low-to-high priority order.
    pub providers: Vec<VfsProviderRecord>,
    /// Index into `providers` that wins.
    pub winner_index: usize,
}

/// Report of all keys with more than one provider.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct DuplicateReport {
    /// Duplicate entries sorted by key.
    pub entries: Vec<DuplicateEntry>,
}

/// Summary for one archive loaded into the VFS.
///
/// This report type is available without archive features so callers can use one API surface, but
/// rows are only populated when archives were actually loaded through `beth-archives`/`zip` support.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ArchiveInfo {
    /// Source index of the archive.
    pub source_index: usize,
    /// Archive path on disk.
    pub path: PathBuf,
    /// Number of entries provided by this archive.
    pub entry_count: usize,
    /// Number of archive entries that win in the resolved VFS.
    pub winning_entry_count: usize,
}

/// One archive entry known to the VFS.
///
/// This report type is available without archive features so callers can use one API surface, but
/// entries are only populated when archives were actually loaded through `beth-archives`/`zip` support.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ArchiveEntry {
    /// Normalized VFS key.
    pub key: PathBuf,
    /// Archive path on disk.
    pub archive_path: PathBuf,
    /// Original in-archive entry path.
    pub original_path: PathBuf,
    /// Whether this archive entry is the resolved winner.
    pub wins: bool,
}

/// Providers whose distinct original paths normalize to one key.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct CaseCollision {
    /// Normalized VFS key.
    pub key: PathBuf,
    /// Providers participating in the collision.
    pub providers: Vec<VfsProviderRecord>,
}

/// Report of case/path-normalization collisions.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct CaseCollisionReport {
    /// Collisions sorted by key.
    pub collisions: Vec<CaseCollision>,
}

/// Structural validation issue for a VFS.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum ValidationIssue {
    /// Loose winner no longer exists on disk.
    MissingLooseSource {
        /// Normalized VFS key whose loose source is missing.
        key: PathBuf,
        /// Missing loose source path.
        source: PathBuf,
    },
    /// A file key would block materializing another key as a child path.
    FileDirectoryConflict {
        /// Key that would need to be materialized as a file.
        file_key: PathBuf,
        /// Key that would need the file key to be a directory prefix.
        directory_key: PathBuf,
    },
    /// Distinct original paths normalize to the same VFS key.
    CaseCollision {
        /// Normalized key shared by colliding providers.
        key: PathBuf,
        /// Original provider paths that normalize to `key`.
        providers: Vec<PathBuf>,
    },
}

/// Structural validation report for a VFS.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ValidationReport {
    /// Validation issues discovered.
    pub issues: Vec<ValidationIssue>,
}

/// Action that materialization would perform.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum MaterializationAction {
    /// Create a hardlink.
    Hardlink {
        /// Normalized VFS key to materialize.
        key: PathBuf,
        /// Loose source path to link from.
        source: PathBuf,
        /// Destination path to create.
        dest: PathBuf,
    },
    /// Create a symbolic link.
    Symlink {
        /// Normalized VFS key to materialize.
        key: PathBuf,
        /// Loose source path to link from.
        source: PathBuf,
        /// Destination path to create.
        dest: PathBuf,
    },
    /// Copy a loose file.
    Copy {
        /// Normalized VFS key to materialize.
        key: PathBuf,
        /// Loose source path to copy from.
        source: PathBuf,
        /// Destination path to create.
        dest: PathBuf,
    },
    /// Extract an archive entry.
    ExtractArchive {
        /// Normalized VFS key to materialize.
        key: PathBuf,
        /// Archive path to extract from.
        archive: PathBuf,
        /// Destination path to create.
        dest: PathBuf,
    },
    /// Skip an archive file or entry.
    SkipArchiveFile {
        /// Normalized VFS key that would otherwise materialize archive data.
        key: PathBuf,
        /// Archive path that was skipped.
        archive: PathBuf,
    },
}

/// Issue that materialization planning can detect without writing files.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum MaterializationIssue {
    /// Loose source is missing.
    MissingLooseSource {
        /// Normalized VFS key whose source is missing.
        key: PathBuf,
        /// Missing loose source path.
        source: PathBuf,
    },
    /// Planned destination has a file/directory conflict.
    FileDirectoryConflict {
        /// Normalized VFS key that cannot be materialized safely.
        key: PathBuf,
        /// Conflicting destination path.
        dest: PathBuf,
    },
    /// Planned destination would escape the output root or hit an unsafe path.
    ///
    /// The current dry-run planner only receives a destination root and normalized VFS keys, so this
    /// variant is reserved for destination-aware safety checks added without changing the report
    /// shape. Execution paths still perform their own root/parent safety checks before writing.
    UnsafeDestination {
        /// Normalized VFS key with an unsafe destination.
        key: PathBuf,
        /// Unsafe destination path.
        dest: PathBuf,
    },
}

/// Dry-run plan for VFS materialization.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct MaterializationPlan {
    /// Planned actions.
    pub actions: Vec<MaterializationAction>,
    /// Issues found while planning.
    pub issues: Vec<MaterializationIssue>,
}

impl VFS {
    fn provider_record_from_entry(
        key: &NormalizedPath,
        entry: &super::ProviderEntry,
    ) -> VfsProviderRecord {
        let key_path = key_to_path_buf_lossy(key);
        let source = entry.provider.source.clone();
        let original_path = VFS::provider_original_path(&source, key, &entry.provider.file);
        let resolved_path = if source.kind == SourceKind::Archive {
            format!("{}::{}", source.path.display(), original_path.display())
        } else {
            source.path.join(&original_path).display().to_string()
        };
        VfsProviderRecord {
            source_index: entry.source_index,
            source,
            key: key_path,
            original_path,
            resolved_path,
        }
    }

    fn provider_records_for_key(&self, key: &NormalizedPath) -> Vec<VfsProviderRecord> {
        self.providers.get(key).map_or_else(Vec::new, |entries| {
            entries
                .iter()
                .map(|entry| Self::provider_record_from_entry(key, entry))
                .collect()
        })
    }

    /// Return provider report records for a normalized key in low-to-high priority order.
    #[must_use]
    pub fn provider_records_for<K: crate::VfsKeyInput + ?Sized>(
        &self,
        path: &K,
    ) -> Vec<VfsProviderRecord> {
        let key = path.to_vfs_key();
        self.provider_records_for_key(&key)
    }

    /// Explain why `path` resolves to its current winner.
    #[must_use]
    pub fn explain<K: crate::VfsKeyInput + ?Sized>(&self, path: &K) -> Option<ExplainReport> {
        let key = path.to_vfs_key();
        let mut providers = self.provider_records_for_key(&key);
        let winner = providers.pop()?;
        Some(ExplainReport {
            key: key_to_path_buf_lossy(&key),
            winner,
            overridden: providers,
        })
    }

    /// Return all normalized keys that have more than one provider.
    #[must_use]
    pub fn duplicates(&self) -> DuplicateReport {
        self.duplicates_matching_key(|_| true)
    }

    /// Return duplicate keys whose normalized VFS key matches `pattern`.
    ///
    /// The regex is compiled case-insensitively and matched against normalized VFS keys using `/`
    /// separators. This filters the same duplicate rows returned by [`VFS::duplicates`]; it does
    /// not inspect physical source paths or provider paths.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `pattern` is not a valid regex.
    pub fn duplicates_matching_regex(
        &self,
        pattern: &str,
    ) -> std::result::Result<DuplicateReport, regex::Error> {
        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()?;
        Ok(self.duplicates_matching_key(|key| re.is_match(&key_to_string_lossy(key))))
    }

    fn duplicates_matching_key(
        &self,
        matches_key: impl Fn(&NormalizedPath) -> bool,
    ) -> DuplicateReport {
        let mut entries: Vec<_> = self
            .providers
            .keys()
            .filter(|key| matches_key(key))
            .cloned()
            .filter_map(|key| {
                let providers = self.provider_records_for_key(&key);
                (providers.len() > 1).then(|| DuplicateEntry {
                    key: key_to_path_buf_lossy(&key),
                    winner_index: providers.len() - 1,
                    providers,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        DuplicateReport { entries }
    }

    /// Return loaded archive sources and their contribution counts.
    #[must_use]
    pub fn archives(&self) -> Vec<ArchiveInfo> {
        let mut counts: AHashMap<usize, (usize, usize)> = AHashMap::new();
        for providers in self.providers.values() {
            let Some(winner_index) = providers.len().checked_sub(1) else {
                continue;
            };
            for (provider_index, entry) in providers
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.provider.source.kind == SourceKind::Archive)
            {
                let counts = counts.entry(entry.source_index).or_default();
                counts.0 += 1;
                if provider_index == winner_index {
                    counts.1 += 1;
                }
            }
        }
        let mut archives: Vec<_> = self
            .sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.kind == SourceKind::Archive)
            .map(|(source_index, source)| {
                let (entry_count, winning_entry_count) =
                    counts.get(&source_index).copied().unwrap_or_default();
                ArchiveInfo {
                    source_index,
                    path: source.path.clone(),
                    entry_count,
                    winning_entry_count,
                }
            })
            .collect();
        archives.sort_by(|a, b| a.path.cmp(&b.path));
        archives
    }

    /// Return entries provided by a loaded archive path.
    #[must_use]
    pub fn archive_entries(&self, archive: impl AsRef<Path>) -> Vec<ArchiveEntry> {
        let archive = normalize_host_path(archive.as_ref()).into_owned();
        let mut entries = Vec::new();
        for (key, providers) in &self.providers {
            let Some(winner_index) = providers.len().checked_sub(1) else {
                continue;
            };
            for (provider_index, entry) in providers.iter().enumerate().filter(|(_, entry)| {
                entry.provider.source.kind == SourceKind::Archive
                    && normalize_host_path(&entry.provider.source.path).as_ref()
                        == archive.as_path()
            }) {
                let original_path =
                    VFS::provider_original_path(&entry.provider.source, key, &entry.provider.file);
                entries.push(ArchiveEntry {
                    key: key_to_path_buf_lossy(key),
                    archive_path: entry.provider.source.path.clone(),
                    original_path,
                    wins: provider_index == winner_index,
                });
            }
        }
        entries.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| a.archive_path.cmp(&b.archive_path))
                .then_with(|| a.original_path.cmp(&b.original_path))
                .then_with(|| a.wins.cmp(&b.wins))
        });
        entries
    }

    /// Return normalized VFS keys provided by a loaded archive path.
    #[must_use]
    pub fn files_from_archive(&self, archive: impl AsRef<Path>) -> Vec<PathBuf> {
        self.archive_entries(archive)
            .into_iter()
            .map(|entry| entry.key)
            .collect()
    }

    /// Return distinct original paths that normalize to the same VFS key.
    #[must_use]
    pub fn case_collisions(&self) -> CaseCollisionReport {
        let mut collisions = Vec::new();
        for key in self.providers.keys() {
            let providers = self.provider_records_for_key(key);
            let spellings: AHashSet<_> =
                providers.iter().map(|p| p.original_path.clone()).collect();
            if spellings.len() > 1 {
                collisions.push(CaseCollision {
                    key: key_to_path_buf_lossy(key),
                    providers: providers.clone(),
                });
            }
        }
        collisions.sort_by(|a, b| a.key.cmp(&b.key));
        CaseCollisionReport { collisions }
    }

    /// Return per-source contribution counts from the provider index.
    #[must_use]
    pub fn source_contributions(&self) -> SourceContributionReport {
        self.layer_index.source_contributions()
    }

    /// Validate structural consistency of the resolved VFS and provider index.
    #[must_use]
    pub fn validate(&self) -> ValidationReport {
        let mut issues = Vec::new();
        for (key, file) in &self.file_map {
            if file.is_loose() && !file.path().exists() {
                issues.push(ValidationIssue::MissingLooseSource {
                    key: key_to_path_buf_lossy(key),
                    source: file.path().to_path_buf(),
                });
            }
        }
        let mut keys: Vec<_> = self.file_map.keys().collect();
        keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        for key in &keys {
            let mut child_prefix = key.as_bytes().to_vec();
            child_prefix.push(b'/');
            let child_index =
                keys.partition_point(|candidate| candidate.as_bytes() < child_prefix.as_slice());
            if let Some(child) = keys
                .get(child_index)
                .filter(|candidate| candidate.as_bytes().starts_with(&child_prefix))
            {
                issues.push(ValidationIssue::FileDirectoryConflict {
                    file_key: key_to_path_buf_lossy(key),
                    directory_key: key_to_path_buf_lossy(child),
                });
            }
        }
        ValidationReport { issues }
    }

    /// Return a dry-run plan for materializing this VFS into `dest`.
    #[must_use]
    pub fn materialization_plan(
        &self,
        dest: impl AsRef<Path>,
        opts: &CollapseOptions,
    ) -> MaterializationPlan {
        let dest = dest.as_ref();
        let mut actions = Vec::new();
        let mut issues = Vec::new();
        let mut keys: Vec<_> = self.file_map.keys().cloned().collect();
        keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        let conflicting_files: AHashSet<_> = keys
            .windows(2)
            .filter_map(|window| {
                let [key, candidate] = window else {
                    return None;
                };
                key_is_at_or_under_prefix(candidate, key).then_some(key.clone())
            })
            .collect();
        for key in keys {
            let file = &self.file_map[&key];
            let key_path = key_to_path_buf_lossy(&key);
            let target = dest.join(&key_path);
            if conflicting_files.contains(&key) {
                issues.push(MaterializationIssue::FileDirectoryConflict {
                    key: key_path.clone(),
                    dest: target.clone(),
                });
                continue;
            }
            if file.is_loose() {
                if !file.path().exists() {
                    issues.push(MaterializationIssue::MissingLooseSource {
                        key: key_path.clone(),
                        source: file.path().to_path_buf(),
                    });
                    continue;
                }
                if opts.extract_archives && super::VFS::is_archive_file(file) {
                    actions.push(MaterializationAction::SkipArchiveFile {
                        key: key_path.clone(),
                        archive: file.path().to_path_buf(),
                    });
                } else if opts.use_symlinks {
                    actions.push(MaterializationAction::Symlink {
                        key: key_path.clone(),
                        source: file.path().to_path_buf(),
                        dest: target,
                    });
                } else if opts.allow_copying {
                    actions.push(MaterializationAction::Copy {
                        key: key_path.clone(),
                        source: file.path().to_path_buf(),
                        dest: target,
                    });
                } else {
                    actions.push(MaterializationAction::Hardlink {
                        key: key_path.clone(),
                        source: file.path().to_path_buf(),
                        dest: target,
                    });
                }
            } else if opts.extract_archives {
                actions.push(MaterializationAction::ExtractArchive {
                    key: key_path.clone(),
                    archive: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
                    dest: target,
                });
            } else {
                actions.push(MaterializationAction::SkipArchiveFile {
                    key: key_path,
                    archive: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
                });
            }
        }
        MaterializationPlan { actions, issues }
    }
}
