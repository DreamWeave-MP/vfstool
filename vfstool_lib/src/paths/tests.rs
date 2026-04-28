// SPDX-License-Identifier: GPL-3.0-only
use super::{normalize_host_path, normalize_host_path_in_place};
use std::path::PathBuf;

#[test]
fn normalize_already_normalized_is_noop() {
    let p = "textures/landscape/foo.dds";
    assert_eq!(normalize_host_path(p), PathBuf::from(p));
}

#[test]
fn normalize_backslash_to_forward_slash() {
    assert_eq!(
        normalize_host_path("textures\\landscape\\foo.dds"),
        PathBuf::from("textures/landscape/foo.dds"),
    );
}

#[test]
fn normalize_uppercase_to_lowercase() {
    assert_eq!(
        normalize_host_path("Meshes/Actors/Foo.NIF"),
        PathBuf::from("meshes/actors/foo.nif"),
    );
}

#[test]
fn normalize_windows_path_combined() {
    assert_eq!(
        normalize_host_path("Meshes\\Actors\\XBase_Anim.NIF"),
        PathBuf::from("meshes/actors/xbase_anim.nif"),
    );
}

#[test]
fn normalize_host_path_with_spaces_preserved() {
    assert_eq!(
        normalize_host_path("Data Files\\Morrowind.esm"),
        PathBuf::from("data files/morrowind.esm"),
    );
}

#[test]
fn normalize_empty_path() {
    assert_eq!(normalize_host_path(""), PathBuf::from(""));
}

#[test]
fn normalize_single_component_uppercase() {
    assert_eq!(
        normalize_host_path("Morrowind.ESM"),
        PathBuf::from("morrowind.esm")
    );
}

#[test]
fn normalize_already_lowercase_forward_slash_fast_path() {
    let p = "data files/tribunal.esm";
    assert_eq!(normalize_host_path(p), PathBuf::from(p));
}

#[test]
fn normalize_non_ascii_passthrough() {
    let input = "Textures/Nordström.dds";
    let result = normalize_host_path(input).to_string_lossy().into_owned();
    assert!(
        result.starts_with("textures/"),
        "ASCII prefix should be lowercased"
    );
    assert!(
        result.contains("tröm"),
        "non-ASCII content should be preserved unchanged"
    );
}

#[test]
fn normalize_in_place_noop_when_already_normalized() {
    let original = PathBuf::from("textures/landscape/foo.dds");
    let mut path = original.clone();
    normalize_host_path_in_place(&mut path);
    assert_eq!(path, original);
}

#[test]
fn normalize_in_place_backslash() {
    let mut path = PathBuf::from("textures\\landscape\\foo.dds");
    normalize_host_path_in_place(&mut path);
    assert_eq!(path, PathBuf::from("textures/landscape/foo.dds"));
}

#[test]
fn normalize_in_place_uppercase() {
    let mut path = PathBuf::from("Meshes/Actors/Foo.NIF");
    normalize_host_path_in_place(&mut path);
    assert_eq!(path, PathBuf::from("meshes/actors/foo.nif"));
}

#[test]
fn normalize_in_place_empty_path() {
    let mut path = PathBuf::from("");
    normalize_host_path_in_place(&mut path);
    assert_eq!(path, PathBuf::from(""));
}

#[test]
fn normalize_in_place_matches_allocating_version() {
    let cases: &[&str] = &[
        "Meshes\\Actors\\XBase_Anim.NIF",
        "TEXTURES/LANDSCAPE/foo.dds",
        "already/normalized/path",
        "",
        "Morrowind.ESM",
        "mixed\\Case/Path\\FILE.ext",
        "Data Files\\Tribunal.esm",
        "textures/landscape/foo.dds",
    ];
    for &case in cases {
        let mut in_place = PathBuf::from(case);
        normalize_host_path_in_place(&mut in_place);
        assert_eq!(
            in_place,
            normalize_host_path(case),
            "in_place and allocating versions disagree for input {case:?}",
        );
    }
}

#[test]
#[cfg(any(feature = "beth-archives", feature = "zip"))]
fn archive_keys_reject_absolute_parent_and_drive_paths() {
    assert_eq!(
        crate::archives::normalized_archive_key(b"Textures\\Foo.DDS"),
        Some(crate::NormalizedPath::from("textures/foo.dds"))
    );
    assert!(crate::archives::normalized_archive_key(b"../foo.dds").is_none());
    assert!(crate::archives::normalized_archive_key(b"/foo.dds").is_none());
    assert!(crate::archives::normalized_archive_key(b"C:\\foo.dds").is_none());
    assert!(crate::archives::normalized_archive_key(b"c:/foo.dds").is_none());
}
