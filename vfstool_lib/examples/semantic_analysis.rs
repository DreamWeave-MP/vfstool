// SPDX-License-Identifier: MIT OR Apache-2.0
use std::path::Path;

use vfstool_lib::{AssetClass, SemanticDelta, analyze_pair};

fn main() {
    let (class, delta) = analyze_pair(
        Path::new("settings.ini"),
        b"[video]\nresolution = 1920x1080\nfullscreen = true\n",
        b"# same values, different order\n[video]\nfullscreen = true\nresolution = 1920x1080\n",
    );

    assert_eq!(class, AssetClass::Ini);
    assert_eq!(delta, SemanticDelta::CosmeticOnly);
    println!("{class:?}: {delta:?}");
}
