// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::normalize_path;
use std::path::Path;

/// Match a normalized VFS key against a glob pattern.
#[must_use]
pub fn path_glob_matches(glob: &str, path: &Path) -> bool {
    glob_match_string(glob, &normalize_path(path).to_string_lossy())
}

/// Match a source path against a glob pattern.
#[must_use]
pub fn source_glob_matches(glob: &str, source_path: &Path) -> bool {
    glob_match_string(glob, &normalize_path(source_path).to_string_lossy())
}

fn glob_match_string(glob: &str, text: &str) -> bool {
    let mut regex_pattern = String::from("^");

    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if i + 1 < chars.len() && chars[i + 1] == '*' {
                    regex_pattern.push_str(".*");
                    i += 2;
                } else {
                    regex_pattern.push_str("[^/]*");
                    i += 1;
                }
            }
            '?' => {
                regex_pattern.push('.');
                i += 1;
            }
            c => {
                regex_pattern.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }

    regex_pattern.push('$');
    regex::Regex::new(&regex_pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
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
}
