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
    pub fn evaluate(&self, index: &LayerIndex, vfs: &VFS) -> io::Result<PolicyResult> {
        let mut keys: Vec<PathBuf> = vfs.iter().map(|(key, _)| key.clone()).collect();
        keys.sort();
        let mut violations = Vec::new();

        for rule in &self.rules {
            evaluate_rule(rule, index, vfs, &keys, &mut violations)?;
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

fn evaluate_rule(
    rule: &Rule,
    index: &LayerIndex,
    vfs: &VFS,
    keys: &[PathBuf],
    violations: &mut Vec<Violation>,
) -> io::Result<()> {
    match rule {
        Rule::WinnerMustMatch {
            path_glob,
            source_glob,
        } => evaluate_winner_source_rule(
            index,
            vfs,
            keys,
            WinnerSourceRule {
                rule_name: "winner_must_match",
                path_glob,
                source_glob,
                expect_match: true,
            },
            violations,
        ),
        Rule::WinnerMustNotMatch {
            path_glob,
            source_glob,
        } => evaluate_winner_source_rule(
            index,
            vfs,
            keys,
            WinnerSourceRule {
                rule_name: "winner_must_not_match",
                path_glob,
                source_glob,
                expect_match: false,
            },
            violations,
        ),
        Rule::MustExist { path_glob } => evaluate_must_exist(keys, path_glob, violations),
        Rule::MustBeUnique { path_glob } => {
            evaluate_provider_count(index, keys, path_glob, None, violations)
        }
        Rule::WinnerKindMustBe { path_glob, kind } => {
            evaluate_winner_kind(index, vfs, keys, path_glob, *kind, violations)
        }
        Rule::MaxOverrideDepth { path_glob, max } => {
            evaluate_provider_count(index, keys, path_glob, Some(*max), violations)
        }
    }
}

#[derive(Clone, Copy)]
struct WinnerSourceRule<'a> {
    rule_name: &'static str,
    path_glob: &'a str,
    source_glob: &'a str,
    expect_match: bool,
}

fn evaluate_winner_source_rule(
    index: &LayerIndex,
    vfs: &VFS,
    keys: &[PathBuf],
    rule: WinnerSourceRule<'_>,
    violations: &mut Vec<Violation>,
) -> io::Result<()> {
    let path_glob = compile_glob("path_glob", rule.path_glob)?;
    let source_glob = compile_glob("source_glob", rule.source_glob)?;
    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
        let Some(prov) = index.provenance(vfs, key, false)? else {
            continue;
        };
        let matched = source_glob.is_match(&prov.winner.path);
        if matched != rule.expect_match {
            let message = if rule.expect_match {
                format!(
                    "winner '{}' does not match source glob '{}'",
                    prov.winner.path.display(),
                    rule.source_glob
                )
            } else {
                format!(
                    "winner '{}' matches forbidden source glob '{}'",
                    prov.winner.path.display(),
                    rule.source_glob
                )
            };
            violations.push(error_violation(rule.rule_name, Some(key.clone()), message));
        }
    }
    Ok(())
}

fn evaluate_must_exist(
    keys: &[PathBuf],
    path_glob_text: &str,
    violations: &mut Vec<Violation>,
) -> io::Result<()> {
    let path_glob = compile_glob("path_glob", path_glob_text)?;
    if !keys.iter().any(|k| path_glob.is_match(k)) {
        violations.push(error_violation(
            "must_exist",
            None,
            format!("no key matched '{path_glob_text}'"),
        ));
    }
    Ok(())
}

fn evaluate_provider_count(
    index: &LayerIndex,
    keys: &[PathBuf],
    path_glob_text: &str,
    max: Option<usize>,
    violations: &mut Vec<Violation>,
) -> io::Result<()> {
    let path_glob = compile_glob("path_glob", path_glob_text)?;
    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
        let provider_count = index.sources_containing(key).len();
        match max {
            None if provider_count > 1 => violations.push(error_violation(
                "must_be_unique",
                Some(key.clone()),
                format!("key has {provider_count} providers"),
            )),
            Some(max) if provider_count > max => violations.push(error_violation(
                "max_override_depth",
                Some(key.clone()),
                format!("provider_count {provider_count} exceeds max {max}"),
            )),
            _ => {}
        }
    }
    Ok(())
}

fn evaluate_winner_kind(
    index: &LayerIndex,
    vfs: &VFS,
    keys: &[PathBuf],
    path_glob_text: &str,
    kind: SourceKind,
    violations: &mut Vec<Violation>,
) -> io::Result<()> {
    let path_glob = compile_glob("path_glob", path_glob_text)?;
    for key in keys.iter().filter(|k| path_glob.is_match(k)) {
        let Some(prov) = index.provenance(vfs, key, false)? else {
            continue;
        };
        if prov.winner.kind != kind {
            violations.push(error_violation(
                "winner_kind_must_be",
                Some(key.clone()),
                format!(
                    "winner kind mismatch: expected {:?}, got {:?}",
                    kind, prov.winner.kind
                ),
            ));
        }
    }
    Ok(())
}

fn error_violation(rule: &str, key: Option<PathBuf>, message: String) -> Violation {
    Violation {
        rule: rule.into(),
        key,
        message,
        severity: Severity::Error,
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
