// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::{
    VFS,
    analysis::{LayerIndex, SourceKind},
    matchers::CompiledGlob,
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
        let mut keys: Vec<PathBuf> = vfs.iter().map(|(key, _)| key.clone()).collect();
        keys.sort();
        let mut violations = Vec::new();

        for rule in &self.rules {
            match rule {
                Rule::WinnerMustMatch {
                    path_glob,
                    source_glob,
                } => {
                    let path_glob = compile_glob("path_glob", path_glob)?;
                    let source_glob_text = source_glob;
                    let source_glob = compile_glob("source_glob", source_glob_text)?;
                    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
                        let Some(prov) = index.provenance(vfs, key, false)? else {
                            continue;
                        };
                        if !source_glob.is_match(&prov.winner.path) {
                            violations.push(Violation {
                                rule: "winner_must_match".into(),
                                key: Some(key.clone()),
                                message: format!(
                                    "winner '{}' does not match source glob '{}'",
                                    prov.winner.path.display(),
                                    source_glob_text
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
                    let path_glob = compile_glob("path_glob", path_glob)?;
                    let source_glob_text = source_glob;
                    let source_glob = compile_glob("source_glob", source_glob_text)?;
                    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
                        let Some(prov) = index.provenance(vfs, key, false)? else {
                            continue;
                        };
                        if source_glob.is_match(&prov.winner.path) {
                            violations.push(Violation {
                                rule: "winner_must_not_match".into(),
                                key: Some(key.clone()),
                                message: format!(
                                    "winner '{}' matches forbidden source glob '{}'",
                                    prov.winner.path.display(),
                                    source_glob_text
                                ),
                                severity: Severity::Error,
                            });
                        }
                    }
                }
                Rule::MustExist { path_glob } => {
                    let path_glob_text = path_glob;
                    let path_glob = compile_glob("path_glob", path_glob)?;
                    let exists = keys.iter().any(|k| path_glob.is_match(k));
                    if !exists {
                        violations.push(Violation {
                            rule: "must_exist".into(),
                            key: None,
                            message: format!("no key matched '{path_glob_text}'"),
                            severity: Severity::Error,
                        });
                    }
                }
                Rule::MustBeUnique { path_glob } => {
                    let path_glob = compile_glob("path_glob", path_glob)?;
                    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
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
                    let path_glob = compile_glob("path_glob", path_glob)?;
                    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
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
                    let path_glob = compile_glob("path_glob", path_glob)?;
                    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
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

fn compile_glob(field: &str, glob: &str) -> io::Result<CompiledGlob> {
    CompiledGlob::new(glob).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {field} '{glob}': {err}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{LayerIndex, SourceMeta};
    use crate::path_glob_matches;
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

        fn write(&self, rel: &str, data: &[u8]) {
            let target = self.0.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("failed to create parent dir");
            }
            fs::write(target, data).expect("failed to write file");
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

    #[test]
    fn must_exist_uses_actual_vfs_keys() {
        let index = LayerIndex::from_file_lists(vec![(
            SourceMeta {
                path: PathBuf::from("/a"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("missing.txt")],
        )]);
        let vfs = VFS::new();
        let policy = Policy {
            rules: vec![Rule::MustExist {
                path_glob: "missing.txt".into(),
            }],
        };

        let result = policy.evaluate(&index, &vfs).expect("policy should run");
        assert_eq!(result.violations.len(), 1);
    }

    #[test]
    fn winner_rules_use_actual_vfs_winner() {
        let low = TempDir::new("policy_actual_winner_low");
        let high = TempDir::new("policy_actual_winner_high");
        low.write("shared.txt", b"low");

        let index = LayerIndex::from_file_lists(vec![
            (
                SourceMeta {
                    path: low.path().to_path_buf(),
                    kind: SourceKind::LooseDir,
                },
                vec![PathBuf::from("shared.txt")],
            ),
            (
                SourceMeta {
                    path: high.path().to_path_buf(),
                    kind: SourceKind::LooseDir,
                },
                vec![PathBuf::from("shared.txt")],
            ),
        ]);
        let vfs = VFS::from_directories([low.path()], None::<Vec<&str>>);
        let policy = Policy {
            rules: vec![Rule::WinnerMustMatch {
                path_glob: "shared.txt".into(),
                source_glob: format!("{}", low.path().display()),
            }],
        };

        let result = policy.evaluate(&index, &vfs).expect("policy should run");
        assert!(result.violations.is_empty());
    }
}
