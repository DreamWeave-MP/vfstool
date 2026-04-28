// SPDX-License-Identifier: GPL-3.0-only
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use vfstool_lib::{
    SerializeType, VFS, VFS_LOCK_SCHEMA_VERSION, VfsLock, serde, serde_json, serde_yaml,
    serialize_value, toml,
};

use crate::cli::OutputFormat;

pub fn write_serialized<T: serde::Serialize>(
    path: Option<PathBuf>,
    format: OutputFormat,
    value: &T,
) -> io::Result<()> {
    let serialized = serialize_value(value, output_to_serialize_type(format))?;
    match path {
        None => write_stdout(&serialized)?,
        Some(p) => {
            if let Some(parent) = p.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!(
                            "failed to create output directory '{}': {e}",
                            parent.display()
                        ),
                    )
                })?;
            }
            write!(
                fs::File::create(&p).map_err(|e| io::Error::new(
                    e.kind(),
                    format!("failed to create output file '{}': {e}", p.display())
                ))?,
                "{serialized}"
            )?;
        }
    }
    Ok(())
}

pub fn parse_lock_file(path: &Path) -> io::Result<VfsLock> {
    let content = fs::read_to_string(path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("failed to read lock file '{}': {e}", path.display()),
        )
    })?;
    let lock: VfsLock = match path.extension().and_then(std::ffi::OsStr::to_str) {
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
    }?;

    if lock.schema_version != VFS_LOCK_SCHEMA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported VFS lock schema_version {} in '{}'; expected {}",
                lock.schema_version,
                path.display(),
                VFS_LOCK_SCHEMA_VERSION
            ),
        ));
    }

    Ok(lock)
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
        None => write_stdout(&serialized)?,
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!(
                            "failed to create output directory '{}': {e}",
                            parent.display()
                        ),
                    )
                })?;
            }
            let mut file = fs::File::create(&path).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("failed to create output file '{}': {e}", path.display()),
                )
            })?;
            write!(file, "{serialized}")?;
        }
    }
    Ok(())
}

fn write_stdout(serialized: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    match writeln!(stdout, "{serialized}") {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e),
    }
}
