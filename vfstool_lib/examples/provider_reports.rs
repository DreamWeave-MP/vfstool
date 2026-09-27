// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{path::Path, path::PathBuf};

use vfstool_lib::{LayerIndex, SourceKind, SourceMeta};

fn main() {
    let layer = LayerIndex::from_file_lists([
        (
            SourceMeta {
                path: PathBuf::from("base"),
                kind: SourceKind::LooseDir,
            },
            vec![PathBuf::from("textures/foo.dds")],
        ),
        (
            SourceMeta {
                path: PathBuf::from("overhaul"),
                kind: SourceKind::LooseDir,
            },
            vec![
                PathBuf::from("textures/foo.dds"),
                PathBuf::from("meshes/bar.nif"),
            ],
        ),
    ]);

    for provider in layer.provider_chain(Path::new("textures/foo.dds")) {
        println!(
            "source #{} {} provides {}",
            provider.source_index,
            provider.source.path.display(),
            provider.key.display()
        );
    }

    let duplicate_keys = layer.duplicate_keys();
    println!("duplicate keys: {}", duplicate_keys.len());
}
