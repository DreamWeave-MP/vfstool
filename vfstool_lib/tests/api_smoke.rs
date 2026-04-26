// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use vfstool_lib::{
    CollapseOptions, ConflictIndex, ContentDigest, LayerIndex, MutableVfs, NormalizedKey, SourceId,
    SourceKind, SourceMeta, VFS, VfsFile, VfsProvider, changed_files, experimental, normalize_path,
    normalize_path_in_place, path_glob_matches, semantic,
};

#[test]
fn root_reexports_remain_usable() {
    let mut normalized = PathBuf::from("Textures\\Foo.DDS");
    normalize_path_in_place(&mut normalized);

    assert_eq!(
        normalize_path("Textures\\Foo.DDS"),
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
        vfs.insert_loose_file("Textures/Foo.DDS", &physical)
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
    let provider = VfsProvider {
        source: source.clone(),
        file: file.clone(),
    };
    let mut mutable = MutableVfs::new();
    assert!(mutable.push_provider("textures/foo.dds", provider));
    assert!(mutable.to_vfs().contains(Path::new("textures/foo.dds")));

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
