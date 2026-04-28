// SPDX-License-Identifier: GPL-3.0-only
use std::path::PathBuf;

use vfstool_lib::{SourceKind, SourceMeta, VFS, VfsFile, VfsProvider};

fn provider(source: &str, file: &str) -> VfsProvider {
    VfsProvider::new(
        SourceMeta {
            path: PathBuf::from(source),
            kind: SourceKind::LooseDir,
        },
        VfsFile::from(file),
    )
}

fn main() {
    let mut vfs = VFS::new();
    vfs.push_provider(
        "textures/foo.dds",
        provider("base", "/mods/base/textures/foo.dds"),
    );
    vfs.push_provider(
        "textures/foo.dds",
        provider("overhaul", "/mods/overhaul/textures/foo.dds"),
    );

    let providers = vfs
        .providers_for("textures/foo.dds")
        .expect("providers should exist");
    assert_eq!(
        providers.last().unwrap().source.path,
        PathBuf::from("overhaul")
    );

    vfs.remove_winner("textures/foo.dds");
    let providers = vfs
        .providers_for("textures/foo.dds")
        .expect("base provider should be revealed");
    assert_eq!(providers.last().unwrap().source.path, PathBuf::from("base"));
}
