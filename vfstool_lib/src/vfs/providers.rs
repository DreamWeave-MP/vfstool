// SPDX-License-Identifier: MIT OR Apache-2.0
use super::VFS;
use crate::{CollapseOptions, SourceKind, SourceMeta, normalize_path};
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

/// Contribution counts for one source.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SourceContribution {
    /// Source index in low-to-high priority order.
    pub source_index: usize,
    /// Source metadata.
    pub source: SourceMeta,
    /// Number of files from this source that win.
    pub winning_files: usize,
    /// Number of files from this source overridden by later sources.
    pub overridden_files: usize,
    /// Number of files unique to this source.
    pub unique_files: usize,
    /// Number of files that share a key with another provider.
    pub duplicate_files: usize,
    /// Number of loose-file providers from this source.
    pub loose_files: usize,
    /// Number of archive-entry providers from this source.
    pub archive_files: usize,
}

/// Contribution report for every source.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct SourceContributionReport {
    /// Per-source contribution rows.
    pub sources: Vec<SourceContribution>,
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
    fn provider_record_for(&self, key: &Path, source_index: usize) -> Option<VfsProviderRecord> {
        let source = self.layer_index.sources.get(source_index)?.clone();
        let original_path = self
            .layer_index
            .provider_original_path(source_index, key)
            .map_or_else(|| key.to_path_buf(), Path::to_path_buf);
        let resolved_path = if source.kind == SourceKind::Archive {
            format!("{}::{}", source.path.display(), original_path.display())
        } else {
            source.path.join(&original_path).display().to_string()
        };
        Some(VfsProviderRecord {
            source_index,
            source,
            key: key.to_path_buf(),
            original_path,
            resolved_path,
        })
    }

    fn provider_records_for_key(&self, key: &Path) -> Vec<VfsProviderRecord> {
        self.layer_index
            .sources_containing(key)
            .iter()
            .filter_map(|&source_index| self.provider_record_for(key, source_index))
            .collect()
    }

    /// Return providers for a normalized key in low-to-high priority order.
    #[must_use]
    pub fn providers_for(&self, path: impl AsRef<Path>) -> Vec<VfsProviderRecord> {
        let key = normalize_path(path.as_ref()).into_owned();
        self.provider_records_for_key(&key)
    }

    /// Explain why `path` resolves to its current winner.
    #[must_use]
    pub fn explain(&self, path: impl AsRef<Path>) -> Option<ExplainReport> {
        let key = normalize_path(path.as_ref()).into_owned();
        let mut providers = self.provider_records_for_key(&key);
        let winner = providers.pop()?;
        Some(ExplainReport {
            key,
            winner,
            overridden: providers,
        })
    }

    /// Return all normalized keys that have more than one provider.
    #[must_use]
    pub fn duplicates(&self) -> DuplicateReport {
        let mut entries: Vec<_> = self
            .layer_index
            .keys()
            .into_iter()
            .filter_map(|key| {
                let providers = self.provider_records_for_key(&key);
                (providers.len() > 1).then(|| DuplicateEntry {
                    key,
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
        for key in self.layer_index.keys() {
            let providers = self.provider_records_for_key(&key);
            let Some(winner) = providers.last() else {
                continue;
            };
            for provider in providers
                .iter()
                .filter(|p| p.source.kind == SourceKind::Archive)
            {
                let counts = counts.entry(provider.source_index).or_default();
                counts.0 += 1;
                if provider.source_index == winner.source_index {
                    counts.1 += 1;
                }
            }
        }
        let mut archives: Vec<_> = self
            .layer_index
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
        let archive = normalize_path(archive.as_ref()).into_owned();
        let mut entries = Vec::new();
        for key in self.layer_index.keys() {
            let providers = self.provider_records_for_key(&key);
            let Some(winner) = providers.last() else {
                continue;
            };
            for provider in providers.iter().filter(|p| {
                p.source.kind == SourceKind::Archive
                    && normalize_path(&p.source.path).as_ref() == archive.as_path()
            }) {
                entries.push(ArchiveEntry {
                    key: key.clone(),
                    archive_path: provider.source.path.clone(),
                    original_path: provider.original_path.clone(),
                    wins: provider.source_index == winner.source_index,
                });
            }
        }
        entries.sort_by(|a, b| a.key.cmp(&b.key));
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
        for key in self.layer_index.keys() {
            let providers = self.provider_records_for_key(&key);
            let spellings: AHashSet<_> =
                providers.iter().map(|p| p.original_path.clone()).collect();
            if spellings.len() > 1 {
                collisions.push(CaseCollision {
                    key,
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
        let mut rows: Vec<_> = self
            .layer_index
            .sources
            .iter()
            .enumerate()
            .map(|(source_index, source)| SourceContribution {
                source_index,
                source: source.clone(),
                winning_files: 0,
                overridden_files: 0,
                unique_files: 0,
                duplicate_files: 0,
                loose_files: 0,
                archive_files: 0,
            })
            .collect();
        for key in self.layer_index.keys() {
            let providers = self.provider_records_for_key(&key);
            let Some(winner) = providers.last() else {
                continue;
            };
            let is_unique = providers.len() == 1;
            let winner_source_index = winner.source_index;
            for provider in &providers {
                let row = &mut rows[provider.source_index];
                if provider.source.kind == SourceKind::Archive {
                    row.archive_files += 1;
                } else {
                    row.loose_files += 1;
                }
                if is_unique {
                    row.unique_files += 1;
                } else {
                    row.duplicate_files += 1;
                }
                if provider.source_index == winner_source_index {
                    row.winning_files += 1;
                } else {
                    row.overridden_files += 1;
                }
            }
        }
        SourceContributionReport { sources: rows }
    }

    /// Validate structural consistency of the resolved VFS and provider index.
    #[must_use]
    pub fn validate(&self) -> ValidationReport {
        let mut issues = Vec::new();
        for (key, file) in &self.file_map {
            if file.is_loose() && !file.path().exists() {
                issues.push(ValidationIssue::MissingLooseSource {
                    key: key.clone(),
                    source: file.path().to_path_buf(),
                });
            }
        }
        let keys: Vec<_> = self.file_map.keys().collect();
        for key in &keys {
            let prefix = format!("{}/", key.display());
            if let Some(child) = keys
                .iter()
                .find(|candidate| candidate.to_string_lossy().starts_with(&prefix))
            {
                issues.push(ValidationIssue::FileDirectoryConflict {
                    file_key: (*key).clone(),
                    directory_key: (*child).clone(),
                });
            }
        }
        for collision in self.case_collisions().collisions {
            issues.push(ValidationIssue::CaseCollision {
                key: collision.key,
                providers: collision
                    .providers
                    .into_iter()
                    .map(|p| p.original_path)
                    .collect(),
            });
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
        let keys: Vec<_> = self.file_map.keys().cloned().collect();
        for (key, file) in &self.file_map {
            let target = dest.join(key);
            let child_prefix = format!("{}/", key.display());
            if keys
                .iter()
                .any(|candidate| candidate.to_string_lossy().starts_with(&child_prefix))
            {
                issues.push(MaterializationIssue::FileDirectoryConflict {
                    key: key.clone(),
                    dest: target.clone(),
                });
                continue;
            }
            if file.is_loose() {
                if !file.path().exists() {
                    issues.push(MaterializationIssue::MissingLooseSource {
                        key: key.clone(),
                        source: file.path().to_path_buf(),
                    });
                    continue;
                }
                if opts.extract_archives && super::VFS::is_archive_file(file) {
                    actions.push(MaterializationAction::SkipArchiveFile {
                        key: key.clone(),
                        archive: file.path().to_path_buf(),
                    });
                } else if opts.use_symlinks {
                    actions.push(MaterializationAction::Symlink {
                        key: key.clone(),
                        source: file.path().to_path_buf(),
                        dest: target,
                    });
                } else if opts.allow_copying {
                    actions.push(MaterializationAction::Copy {
                        key: key.clone(),
                        source: file.path().to_path_buf(),
                        dest: target,
                    });
                } else {
                    actions.push(MaterializationAction::Hardlink {
                        key: key.clone(),
                        source: file.path().to_path_buf(),
                        dest: target,
                    });
                }
            } else if opts.extract_archives {
                actions.push(MaterializationAction::ExtractArchive {
                    key: key.clone(),
                    archive: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
                    dest: target,
                });
            } else {
                actions.push(MaterializationAction::SkipArchiveFile {
                    key: key.clone(),
                    archive: PathBuf::from(file.parent_archive_path().unwrap_or_default()),
                });
            }
        }
        MaterializationPlan { actions, issues }
    }
}
