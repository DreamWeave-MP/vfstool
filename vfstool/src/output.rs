// SPDX-License-Identifier: GPL-3.0-only
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use vfstool_lib::{SerializeType, VFS, VfsLock, serialize_value};

use crate::cli::OutputFormat;

pub fn write_serialized<T: serde::Serialize>(
    path: Option<PathBuf>,
    format: OutputFormat,
    value: &T,
) -> io::Result<()> {
    let serialized = serialize_value(value, output_to_serialize_type(format))?;
    match path {
        None => println!("{serialized}"),
        Some(p) => {
            if let Some(parent) = p.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            write!(fs::File::create(&p)?, "{serialized}")?;
        }
    }
    Ok(())
}

pub fn parse_lock_file(path: &Path) -> io::Result<VfsLock> {
    let content = fs::read_to_string(path)?;
    match path.extension().and_then(std::ffi::OsStr::to_str) {
        Some("json") => serde_json::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid JSON lock file '{}': {e}", path.display()),
            )
        }),
        Some("toml") => toml::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid TOML lock file '{}': {e}", path.display()),
            )
        }),
        _ => serde_yaml::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid YAML lock file '{}': {e}", path.display()),
            )
        }),
    }
}

fn output_to_serialize_type(format: OutputFormat) -> SerializeType {
    match format {
        OutputFormat::Json => SerializeType::Json,
        OutputFormat::Yaml => SerializeType::Yaml,
        OutputFormat::Toml => SerializeType::Toml,
    }
}

pub fn write_serialized_vfs(
    path: Option<PathBuf>,
    format: OutputFormat,
    files: &vfstool_lib::DisplayTree,
) -> io::Result<()> {
    let serialized = VFS::serialize_from_tree(files, output_to_serialize_type(format))?;
    match path {
        None => println!("{serialized}"),
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)?;
            }
            let mut file = fs::File::create(&path)?;
            write!(file, "{serialized}")?;
        }
    }
    Ok(())
}
