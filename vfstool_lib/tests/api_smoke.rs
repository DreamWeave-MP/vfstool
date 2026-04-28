// SPDX-License-Identifier: GPL-3.0-only
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use vfstool_lib::{
    CollapseOptions, ConflictIndex, ContentDigest, LayerIndex, NormalizedKey, NormalizedPath,
    SourceId, SourceKind, SourceMeta, VFS, VfsFile, VfsKeyInput, VfsProvider, changed_files,
    experimental, normalize_host_path, normalize_host_path_in_place, path_glob_matches, semantic,
};

#[test]
fn root_reexports_remain_usable() {
    let mut normalized = PathBuf::from("Textures\\Foo.DDS");
    normalize_host_path_in_place(&mut normalized);

    assert_eq!(
        normalize_host_path("Textures\\Foo.DDS"),
        PathBuf::from("textures/foo.dds")
    );
    assert_eq!(normalized, PathBuf::from("textures/foo.dds"));
    assert!(path_glob_matches(
        "textures/*.dds",
        Path::new("textures/foo.dds")
    ));

    let mut vfs = VFS::new();
    let physical = PathBuf::from("/tmp/source/textures/foo.dds");
    assert!(
        vfs.set_winner_loose_file("Textures/Foo.DDS", &physical)
            .is_none()
    );
    assert_eq!(vfs.get_file("textures/foo.dds").unwrap().path(), physical);

    let source = SourceMeta {
        path: PathBuf::from("/tmp/source"),
        kind: SourceKind::LooseDir,
    };
    let index =
        LayerIndex::from_file_lists([(source.clone(), vec![PathBuf::from("textures/foo.dds")])]);
    assert_eq!(index.source_by_id(SourceId::from_index(0)), Some(&source));
    assert_eq!(
        index.sources_containing(Path::new("textures/foo.dds")),
        &[0]
    );

    let conflicts = ConflictIndex::from_file_lists([(
        source.path.clone(),
        vec![PathBuf::from("textures/foo.dds")],
    )]);
    assert_eq!(conflicts.sources.len(), 1);
    assert!(vfs.explain(Path::new("textures/foo.dds")).is_some());
    assert_eq!(index.provider_chain(Path::new("textures/foo.dds")).len(), 1);
    let key = NormalizedKey::new("Textures/Foo.DDS");
    assert_eq!(key.as_path(), Path::new("textures/foo.dds"));
    let digest = ContentDigest::blake3([0; 32], 0);
    assert_eq!(digest.size, 0);

    let collapse_options = CollapseOptions {
        allow_copying: true,
        extract_archives: true,
        use_symlinks: false,
    };
    assert!(collapse_options.allow_copying);
    let file = VfsFile::from("/tmp/source/textures/foo.dds");
    let provider = VfsProvider::new(source.clone(), file.clone());
    assert!(vfs.push_provider("textures/foo.dds", provider));
    assert!(vfs.contains(Path::new("textures/foo.dds")));

    let temp_dir = std::env::temp_dir().join(format!("vfstool_api_smoke_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let baseline = HashMap::<PathBuf, [u8; 32]>::new();
    assert!(changed_files(&temp_dir, &baseline).unwrap().is_empty());
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn public_and_experimental_modules_remain_reachable() {
    let (asset_class, semantic_delta) = semantic::analyze_pair(Path::new("foo.txt"), b"a", b"a");
    assert_eq!(asset_class, semantic::AssetClass::Text);
    assert!(matches!(
        semantic_delta,
        semantic::SemanticDelta::NoOpEquivalent
    ));

    let policy = experimental::policy::Policy {
        rules: vec![experimental::policy::Rule::MustExist {
            path_glob: "textures/*.dds".into(),
        }],
    };
    assert_eq!(policy.rules.len(), 1);

    let request = experimental::solve::SolveRequest {
        current_order: Vec::new(),
        constraints: vec![experimental::solve::OrderConstraint::WinnerMustBe {
            path_glob: "textures/*.dds".into(),
            source_glob: "*/source".into(),
        }],
        objective: experimental::solve::SolveObjective::MinMovesFromCurrent,
    };
    assert_eq!(request.constraints.len(), 1);
}

#[test]
fn normalized_path_and_vfs_key_input_reexports_are_usable() {
    let mut vfs = VFS::new();
    let physical = PathBuf::from("/tmp/source/textures/foo.dds");
    let key = NormalizedPath::new(b"Textures\\Foo.DDS");

    assert!(vfs.set_winner_loose_file(&key, &physical).is_none());
    assert_eq!(vfs.get_file(&key).unwrap().path(), physical);

    assert_eq!(
        "Textures\\Foo.DDS".to_vfs_key().as_bytes(),
        b"textures/foo.dds"
    );
    assert_eq!(
        "Textures\\Foo.DDS".to_safe_vfs_key().unwrap().as_bytes(),
        b"textures/foo.dds"
    );
    assert!("../outside.dds".to_safe_vfs_key().is_none());
    assert!("/absolute.dds".to_safe_vfs_key().is_none());
    assert!("C:/absolute.dds".to_safe_vfs_key().is_none());
}

#[cfg(feature = "serialize")]
#[test]
fn serialize_feature_reexports_serialization_crates() {
    fn assert_serialize<T: vfstool_lib::serde::Serialize>(_value: &T) {}

    let value = vfstool_lib::serde_json::json!({ "path": "textures/foo.dds" });
    assert_serialize(&value);

    let yaml = vfstool_lib::serde_yaml::to_string(&value).unwrap();
    assert!(yaml.contains("textures/foo.dds"));

    let toml = vfstool_lib::toml::to_string(&std::collections::BTreeMap::from([(
        "path",
        "textures/foo.dds",
    )]))
    .unwrap();
    assert!(toml.contains("textures/foo.dds"));
}
