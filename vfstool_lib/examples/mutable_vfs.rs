// SPDX-License-Identifier: MIT OR Apache-2.0
use std::path::PathBuf;

use vfstool_lib::{MutableVfs, SourceKind, SourceMeta, VfsFile, VfsProvider};

fn provider(source: &str, file: &str) -> VfsProvider {
    VfsProvider {
        source: SourceMeta {
            path: PathBuf::from(source),
            kind: SourceKind::LooseDir,
        },
        file: VfsFile::from(file),
    }
}

fn main() {
    let mut mutable = MutableVfs::new();
    mutable.push_provider(
        "textures/foo.dds",
        provider("base", "/mods/base/textures/foo.dds"),
    );
    mutable.push_provider(
        "textures/foo.dds",
        provider("overhaul", "/mods/overhaul/textures/foo.dds"),
    );

    let providers = mutable
        .providers_for("textures/foo.dds")
        .expect("providers should exist");
    assert_eq!(
        providers.last().unwrap().source.path,
        PathBuf::from("overhaul")
    );

    mutable.remove_winner("textures/foo.dds");
    let providers = mutable
        .providers_for("textures/foo.dds")
        .expect("base provider should be revealed");
    assert_eq!(providers.last().unwrap().source.path, PathBuf::from("base"));
}
