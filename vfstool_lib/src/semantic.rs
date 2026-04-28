// SPDX-License-Identifier: GPL-3.0-only
use crate::SourceMeta;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Asset family used for semantic analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum AssetClass {
    /// INI-style key-value config files.
    Ini,
    /// TOML config files.
    Toml,
    /// JSON data files.
    Json,
    /// Lua script files.
    LuaScript,
    /// Morrowind-style script source files.
    MwScriptLike,
    /// Generic UTF-8 text.
    Text,
    /// Binary content.
    Binary,
    /// Unknown or unsupported class.
    Unknown,
}

/// Semantic change classification between two versions of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum SemanticDelta {
    /// No semantic or byte-level change.
    NoOpEquivalent,
    /// Semantically equivalent but formatting/comment/order changed.
    CosmeticOnly,
    /// Meaningful behavior/content change.
    BehaviorChanging {
        /// Human-readable change summary.
        change_summary: Vec<String>,
    },
    /// Could not classify.
    Unknown,
}

/// Per-provider relation to winner content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum SemanticRelation {
    /// Byte-identical to winner.
    IdenticalToWinner,
    /// Different bytes from winner.
    DifferentFromWinner,
    /// Content unavailable.
    Unknown,
}

/// Semantic info for one provider.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticProvider {
    /// Source metadata.
    pub source: SourceMeta,
    /// Relation to winning content.
    pub relation: SemanticRelation,
    /// Optional content hash.
    pub hash_blake3: Option<String>,
    /// Optional byte size.
    pub size: Option<u64>,
    /// Optional semantic delta compared to winner content.
    pub semantic_delta_to_winner: Option<SemanticDelta>,
}

/// Semantic conflict for one key with multiple providers.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticConflict {
    /// Normalized key.
    pub key: PathBuf,
    /// Winning source.
    pub winner: SourceMeta,
    /// Providers in low -> high priority order.
    pub providers: Vec<SemanticProvider>,
    /// Inferred asset class.
    pub asset_class: AssetClass,
    /// True if every available hash equals the winner hash.
    pub all_identical: bool,
    /// Count of unique available content hashes.
    pub distinct_versions: usize,
}

/// Semantic conflicts across the load order.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticConflictReport {
    /// One entry per conflicting key.
    pub entries: Vec<SemanticConflict>,
}

/// Archive hashing mode for semantic conflict analysis.
///
/// Loose providers are always hashable through their filesystem paths. Archive providers are gated by
/// this mode because opening and decompressing every overridden archive entry can be expensive on
/// large load orders. When archive hashing is enabled, the analysis opens the exact provider entries
/// already stored in [`VFS`](crate::VFS); it does not reconstruct archive lookup from a lossy
/// archive-path/key pair. Read or decompression failures are returned as I/O errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum ArchiveHashMode {
    /// Do not hash archive providers.
    Disabled,
    /// Hash only archive providers that currently win in the VFS.
    WinnerOnly,
    /// Hash all archive providers present in the VFS provider stack.
    ///
    /// Missing providers remain unknown. Archive entries that are present are opened and hashed;
    /// read/decompression failures are returned as I/O errors instead of being reported as unknown.
    AllProviders,
}

/// Semantic conflict report options.
#[derive(Debug, Clone, Copy)]
pub struct SemanticOpts {
    /// Archive hashing behavior.
    pub archive_hash_mode: ArchiveHashMode,
    /// Include semantic analyzer deltas where possible.
    pub include_semantic_deltas: bool,
}

impl Default for SemanticOpts {
    fn default() -> Self {
        Self {
            archive_hash_mode: ArchiveHashMode::WinnerOnly,
            include_semantic_deltas: false,
        }
    }
}

/// Analyze two file versions and return asset class + semantic delta.
#[must_use]
pub fn analyze_pair(path: &Path, left: &[u8], right: &[u8]) -> (AssetClass, SemanticDelta) {
    let class = infer_asset_class(path, left, right);
    let delta = match class {
        AssetClass::Ini => analyze_ini(left, right),
        AssetClass::Toml => analyze_toml(left, right),
        AssetClass::Json => analyze_json(left, right),
        AssetClass::LuaScript | AssetClass::MwScriptLike | AssetClass::Text => {
            analyze_text(left, right)
        }
        AssetClass::Binary => analyze_binary(left, right),
        AssetClass::Unknown => SemanticDelta::Unknown,
    };
    (class, delta)
}

fn infer_asset_class(path: &Path, left: &[u8], right: &[u8]) -> AssetClass {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);

    match ext.as_deref() {
        Some("ini" | "cfg") => AssetClass::Ini,
        Some("toml") => AssetClass::Toml,
        Some("json") => AssetClass::Json,
        Some("lua") => AssetClass::LuaScript,
        Some("mws" | "mwscript") => AssetClass::MwScriptLike,
        Some("txt" | "md") => AssetClass::Text,
        _ => {
            if is_probably_binary(left) || is_probably_binary(right) {
                AssetClass::Binary
            } else if std::str::from_utf8(left).is_ok() && std::str::from_utf8(right).is_ok() {
                AssetClass::Text
            } else {
                AssetClass::Unknown
            }
        }
    }
}

fn analyze_binary(left: &[u8], right: &[u8]) -> SemanticDelta {
    if left == right {
        SemanticDelta::NoOpEquivalent
    } else {
        SemanticDelta::BehaviorChanging {
            change_summary: vec!["binary payload differs".into()],
        }
    }
}

fn analyze_text(left: &[u8], right: &[u8]) -> SemanticDelta {
    let Ok(left_str) = std::str::from_utf8(left) else {
        return SemanticDelta::Unknown;
    };
    let Ok(right_str) = std::str::from_utf8(right) else {
        return SemanticDelta::Unknown;
    };

    if left_str == right_str {
        return SemanticDelta::NoOpEquivalent;
    }

    if normalize_text_for_comparison(left_str) == normalize_text_for_comparison(right_str) {
        SemanticDelta::CosmeticOnly
    } else {
        SemanticDelta::BehaviorChanging {
            change_summary: vec!["text content differs after normalization".into()],
        }
    }
}

fn normalize_text_for_comparison(input: &str) -> String {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn analyze_ini(left: &[u8], right: &[u8]) -> SemanticDelta {
    let Ok(left_str) = std::str::from_utf8(left) else {
        return SemanticDelta::Unknown;
    };
    let Ok(right_str) = std::str::from_utf8(right) else {
        return SemanticDelta::Unknown;
    };

    if left_str == right_str {
        return SemanticDelta::NoOpEquivalent;
    }

    let left_map = parse_ini_like(left_str);
    let right_map = parse_ini_like(right_str);
    if left_map == right_map {
        SemanticDelta::CosmeticOnly
    } else {
        SemanticDelta::BehaviorChanging {
            change_summary: vec!["INI keys/values differ".into()],
        }
    }
}

fn parse_ini_like(input: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out = BTreeMap::<String, BTreeMap<String, String>>::new();
    let mut current_section = String::from("global");

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            out.entry(current_section.clone())
                .or_default()
                .insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    out
}

fn analyze_toml(left: &[u8], right: &[u8]) -> SemanticDelta {
    // Structured TOML parsing is intentionally tied to the serialize feature,
    // which owns the toml dependency for the crate.
    analyze_structured_pair(left, right, parse_toml_value, "TOML semantic values differ")
}

fn analyze_json(left: &[u8], right: &[u8]) -> SemanticDelta {
    // Structured JSON parsing is intentionally tied to the serialize feature,
    // which owns the serde_json dependency for the crate.
    analyze_structured_pair(left, right, parse_json_value, "JSON semantic values differ")
}

fn analyze_structured_pair<T: PartialEq>(
    left: &[u8],
    right: &[u8],
    parser: fn(&[u8]) -> Option<T>,
    change_message: &str,
) -> SemanticDelta {
    if left == right {
        return SemanticDelta::NoOpEquivalent;
    }

    match (parser(left), parser(right)) {
        (Some(a), Some(b)) => {
            if a == b {
                SemanticDelta::CosmeticOnly
            } else {
                SemanticDelta::BehaviorChanging {
                    change_summary: vec![change_message.into()],
                }
            }
        }
        _ => SemanticDelta::Unknown,
    }
}

#[cfg(feature = "serialize")]
fn parse_toml_value(input: &[u8]) -> Option<toml::Value> {
    let text = std::str::from_utf8(input).ok()?;
    toml::from_str::<toml::Value>(text).ok()
}

#[cfg(not(feature = "serialize"))]
fn parse_toml_value(_input: &[u8]) -> Option<()> {
    None
}

#[cfg(feature = "serialize")]
fn parse_json_value(input: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice::<serde_json::Value>(input).ok()
}

#[cfg(not(feature = "serialize"))]
fn parse_json_value(_input: &[u8]) -> Option<()> {
    None
}

fn is_probably_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_comment_and_order_changes_are_cosmetic() {
        let left = b"[section]\na=1\nb=2\n";
        let right = b"# comment\n[section]\nb=2\na=1\n";
        let (_class, delta) = analyze_pair(Path::new("config.ini"), left, right);
        assert_eq!(delta, SemanticDelta::CosmeticOnly);
    }

    #[test]
    #[cfg(feature = "serialize")]
    fn toml_value_change_is_behavior_changing() {
        let left = b"[x]\na = 1\n";
        let right = b"[x]\na = 2\n";
        let (_class, delta) = analyze_pair(Path::new("config.toml"), left, right);
        assert!(matches!(delta, SemanticDelta::BehaviorChanging { .. }));
    }

    #[test]
    #[cfg(not(feature = "serialize"))]
    fn toml_semantics_are_unknown_without_serialize() {
        let left = b"[x]\na = 1\n";
        let right = b"[x]\na = 2\n";
        let (_class, delta) = analyze_pair(Path::new("config.toml"), left, right);
        assert_eq!(delta, SemanticDelta::Unknown);
    }

    #[test]
    #[cfg(feature = "serialize")]
    fn json_reformat_is_cosmetic() {
        let left = br#"{"a":1,"b":2}"#;
        let right = br#"{
  "b": 2,
  "a": 1
}"#;
        let (_class, delta) = analyze_pair(Path::new("x.json"), left, right);
        assert_eq!(delta, SemanticDelta::CosmeticOnly);
    }

    #[test]
    #[cfg(not(feature = "serialize"))]
    fn json_semantics_are_unknown_without_serialize() {
        let left = br#"{"a":1,"b":2}"#;
        let right = br#"{
  "b": 2,
  "a": 1
}"#;
        let (_class, delta) = analyze_pair(Path::new("x.json"), left, right);
        assert_eq!(delta, SemanticDelta::Unknown);
    }

    #[test]
    fn binary_difference_is_behavior_changing() {
        let left = [0u8, 1, 2];
        let right = [0u8, 1, 3];
        let (_class, delta) = analyze_pair(Path::new("x.bin"), &left, &right);
        assert!(matches!(delta, SemanticDelta::BehaviorChanging { .. }));
    }
}
