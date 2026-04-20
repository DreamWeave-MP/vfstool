// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{
    VFS,
    analysis::{LayerIndex, SourceKind},
    path_glob_matches, source_glob_matches,
};
use std::{io, path::PathBuf};

/// Policy document: ordered rules evaluated against a VFS + layer index.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Rules to evaluate.
    pub rules: Vec<Rule>,
}

/// Severity for policy violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum Severity {
    /// Blocking failure.
    Error,
}

/// Supported policy rules.
#[derive(Debug, Clone)]
pub enum Rule {
    /// Winner source path must match `source_glob` for keys matching `path_glob`.
    WinnerMustMatch {
        /// Glob over normalized VFS keys.
        path_glob: String,
        /// Glob over winning source path strings.
        source_glob: String,
    },
    /// Winner source path must NOT match `source_glob` for keys matching `path_glob`.
    WinnerMustNotMatch {
        /// Glob over normalized VFS keys.
        path_glob: String,
        /// Glob over winning source path strings.
        source_glob: String,
    },
    /// At least one key matching `path_glob` must exist.
    MustExist {
        /// Glob over normalized VFS keys.
        path_glob: String,
    },
    /// Matching keys must have exactly one provider.
    MustBeUnique {
        /// Glob over normalized VFS keys.
        path_glob: String,
    },
    /// Matching keys must be served by the requested winner kind.
    WinnerKindMustBe {
        /// Glob over normalized VFS keys.
        path_glob: String,
        /// Required winner source kind.
        kind: SourceKind,
    },
    /// Matching keys must have <= `max` providers.
    MaxOverrideDepth {
        /// Glob over normalized VFS keys.
        path_glob: String,
        /// Maximum allowed provider count.
        max: usize,
    },
}

/// One policy violation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct Violation {
    /// Rule identifier string.
    pub rule: String,
    /// Optional key associated with the violation.
    pub key: Option<PathBuf>,
    /// Human-readable message.
    pub message: String,
    /// Violation severity.
    pub severity: Severity,
}

/// Full policy evaluation result.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct PolicyResult {
    /// Violations found.
    pub violations: Vec<Violation>,
}

impl Policy {
    /// Evaluate this policy against layer + vfs state.
    ///
    /// # Errors
    ///
    /// Returns an error when provider/provenance resolution fails.
    #[allow(clippy::too_many_lines)]
    pub fn evaluate(&self, index: &LayerIndex, vfs: &VFS) -> io::Result<PolicyResult> {
        let keys = index.keys();
        let mut violations = Vec::new();

        for rule in &self.rules {
            match rule {
                Rule::WinnerMustMatch {
                    path_glob,
                    source_glob,
                } => {
                    for key in keys.iter().filter(|k| path_glob_matches(path_glob, k)) {
                        let Some(prov) = index.provenance(vfs, key, false)? else {
                            continue;
                        };
                        if !source_glob_matches(source_glob, &prov.winner.path) {
                            violations.push(Violation {
                                rule: "winner_must_match".into(),
                                key: Some(key.clone()),
                                message: format!(
                                    "winner '{}' does not match source glob '{}'",
                                    prov.winner.path.display(),
                                    source_glob
                                ),
                                severity: Severity::Error,
                            });
                        }
                    }
                }
                Rule::WinnerMustNotMatch {
                    path_glob,
                    source_glob,
                } => {
                    for key in keys.iter().filter(|k| path_glob_matches(path_glob, k)) {
                        let Some(prov) = index.provenance(vfs, key, false)? else {
                            continue;
                        };
                        if source_glob_matches(source_glob, &prov.winner.path) {
                            violations.push(Violation {
                                rule: "winner_must_not_match".into(),
                                key: Some(key.clone()),
                                message: format!(
                                    "winner '{}' matches forbidden source glob '{}'",
                                    prov.winner.path.display(),
                                    source_glob
                                ),
                                severity: Severity::Error,
                            });
                        }
                    }
                }
                Rule::MustExist { path_glob } => {
                    let exists = keys.iter().any(|k| path_glob_matches(path_glob, k));
                    if !exists {
                        violations.push(Violation {
                            rule: "must_exist".into(),
                            key: None,
                            message: format!("no key matched '{path_glob}'"),
                            severity: Severity::Error,
                        });
                    }
                }
                Rule::MustBeUnique { path_glob } => {
                    for key in keys.iter().filter(|k| path_glob_matches(path_glob, k)) {
                        let provider_count = index.sources_containing(key).len();
                        if provider_count > 1 {
                            violations.push(Violation {
                                rule: "must_be_unique".into(),
                                key: Some(key.clone()),
                                message: format!("key has {provider_count} providers"),
                                severity: Severity::Error,
                            });
                        }
                    }
                }
                Rule::WinnerKindMustBe { path_glob, kind } => {
                    for key in keys.iter().filter(|k| path_glob_matches(path_glob, k)) {
                        let Some(prov) = index.provenance(vfs, key, false)? else {
                            continue;
                        };
                        if prov.winner.kind != *kind {
                            violations.push(Violation {
                                rule: "winner_kind_must_be".into(),
                                key: Some(key.clone()),
                                message: format!(
                                    "winner kind mismatch: expected {:?}, got {:?}",
                                    kind, prov.winner.kind
                                ),
                                severity: Severity::Error,
                            });
                        }
                    }
                }
                Rule::MaxOverrideDepth { path_glob, max } => {
                    for key in keys.iter().filter(|k| path_glob_matches(path_glob, k)) {
                        let provider_count = index.sources_containing(key).len();
                        if provider_count > *max {
                            violations.push(Violation {
                                rule: "max_override_depth".into(),
                                key: Some(key.clone()),
                                message: format!(
                                    "provider_count {provider_count} exceeds max {max}"
                                ),
                                severity: Severity::Error,
                            });
                        }
                    }
                }
            }
        }

        violations.sort_by(|a, b| {
            let ak = a
                .key
                .as_ref()
                .map_or_else(String::new, |k| k.display().to_string());
            let bk = b
                .key
                .as_ref()
                .map_or_else(String::new, |k| k.display().to_string());
            a.rule
                .cmp(&b.rule)
                .then(ak.cmp(&bk))
                .then(a.message.cmp(&b.message))
        });

        Ok(PolicyResult { violations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{LayerIndex, SourceMeta};
    use std::{fs, path::Path};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(name);
            fs::create_dir_all(&dir).expect("failed to create temp dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn glob_match_works() {
        assert!(path_glob_matches(
            "textures/**",
            Path::new("textures/foo/bar.dds")
        ));
        assert!(path_glob_matches(
            "textures/*.dds",
            Path::new("textures/a.dds")
        ));
        assert!(!path_glob_matches(
            "textures/*.dds",
            Path::new("textures/a/b.dds")
        ));
    }

    #[test]
    fn must_exist_reports_missing() {
        let temp = TempDir::new("policy_must_exist_reports_missing");
        let index = LayerIndex::from_file_lists(vec![(
            SourceMeta {
                path: PathBuf::from("/a"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("meshes/a.nif")],
        )]);
        let vfs = VFS::from_directories([temp.path()], None::<Vec<&str>>);

        let policy = Policy {
            rules: vec![Rule::MustExist {
                path_glob: "textures/**".into(),
            }],
        };
        let result = policy
            .evaluate(&index, &vfs)
            .expect("policy evaluate should not fail");
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].rule, "must_exist");
    }
}
