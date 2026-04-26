// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{fs, io, path::PathBuf};

use vfstool_lib::VFS;

fn unique_dir(name: &str) -> io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "vfstool_example_{name}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn main() -> io::Result<()> {
    let base = unique_dir("basic_base")?;
    let replacer = unique_dir("basic_replacer")?;

    fs::create_dir_all(base.join("Textures"))?;
    fs::create_dir_all(replacer.join("textures"))?;
    fs::write(base.join("Textures/Foo.DDS"), b"base texture")?;
    fs::write(replacer.join("textures/foo.dds"), b"replacement texture")?;

    let vfs = VFS::from_directories([&base, &replacer], None);
    let winner = vfs
        .get_file("textures/foo.dds")
        .expect("replacement should win");

    println!("winner: {}", winner.path().display());
    println!("normalized entries: {}", vfs.iter().count());

    fs::remove_dir_all(base)?;
    fs::remove_dir_all(replacer)?;
    Ok(())
}
