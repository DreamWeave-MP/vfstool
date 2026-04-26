// SPDX-License-Identifier: GPL-3.0-only
use crate::normalize_path;
use std::path::Path;

/// A normalized path glob compiled to a regular expression.
#[derive(Debug, Clone)]
pub(crate) struct CompiledGlob(regex::Regex);

impl CompiledGlob {
    /// Compile a glob after normalizing case and separators.
    pub(crate) fn new(glob: &str) -> Result<Self, regex::Error> {
        regex::Regex::new(&glob_regex_pattern(&normalize_path(glob).to_string_lossy())).map(Self)
    }

    /// Match normalized path-like text.
    #[must_use]
    pub(crate) fn is_match(&self, path: &Path) -> bool {
        self.0.is_match(&normalize_path(path).to_string_lossy())
    }
}

/// Match a normalized VFS key against a glob pattern.
#[must_use]
pub fn path_glob_matches(glob: &str, path: &Path) -> bool {
    CompiledGlob::new(glob).is_ok_and(|glob| glob.is_match(path))
}

/// Match a source path against a glob pattern.
#[must_use]
pub fn source_glob_matches(glob: &str, source_path: &Path) -> bool {
    CompiledGlob::new(glob).is_ok_and(|glob| glob.is_match(source_path))
}

fn glob_regex_pattern(glob: &str) -> String {
    let mut regex_pattern = String::from("^");

    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    if i + 2 < chars.len() && chars[i + 2] == '/' {
                        regex_pattern.push_str("(?:.*/)?");
                        i += 3;
                    } else {
                        regex_pattern.push_str(".*");
                        i += 2;
                    }
                } else {
                    regex_pattern.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                regex_pattern.push_str("[^/]");
                i += 1;
            }
            c => {
                regex_pattern.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }

    regex_pattern.push('$');
    regex_pattern
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn path_glob_double_star_matches_nested() {
        assert!(path_glob_matches(
            "textures/**",
            Path::new("textures/foo/bar.dds")
        ));
    }

    #[test]
    fn path_glob_double_star_slash_matches_zero_or_more_directories() {
        assert!(path_glob_matches("**/foo.txt", Path::new("foo.txt")));
        assert!(path_glob_matches("**/foo.txt", Path::new("meshes/foo.txt")));
        assert!(path_glob_matches(
            "textures/**/*.dds",
            Path::new("textures/foo.dds")
        ));
        assert!(path_glob_matches(
            "textures/**/*.dds",
            Path::new("textures/actors/foo.dds")
        ));
    }

    #[test]
    fn path_glob_single_star_does_not_cross_separators() {
        assert!(!path_glob_matches(
            "textures/*.dds",
            Path::new("textures/a/b.dds")
        ));
    }

    #[test]
    fn source_glob_matches_case_insensitive_path_text() {
        assert!(source_glob_matches(
            "**/mods/*patch*",
            PathBuf::from("/home/user/MODS/MyPatch").as_path()
        ));
    }

    #[test]
    fn path_glob_normalizes_pattern_case_and_separators() {
        assert!(path_glob_matches(
            "Textures\\**",
            Path::new("textures/foo.dds")
        ));
    }

    #[test]
    fn question_mark_does_not_cross_separators() {
        assert!(!path_glob_matches(
            "textures/?.dds",
            Path::new("textures/a/b.dds")
        ));
        assert!(path_glob_matches(
            "textures/?.dds",
            Path::new("textures/a.dds")
        ));
    }
}
