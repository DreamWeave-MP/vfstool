// SPDX-License-Identifier: GPL-3.0-only
use std::{fs, io, path::PathBuf};

use vfstool_lib::{ConflictIndex, LayerIndex, VFS};

use crate::exit::VFSToolExitCode;

pub fn load_openmw_config(config_path: PathBuf) -> openmw_config::OpenMWConfiguration {
    match openmw_config::OpenMWConfiguration::new(Some(config_path)) {
        Err(e) => {
            eprintln!("Failed to load configuration file: {e}");
            std::process::exit(VFSToolExitCode::FailedToLoadOpenMWConfig.into());
        }
        Ok(cfg) => cfg,
    }
}

pub fn build_conflict_index(config_path: PathBuf) -> (VFS, ConflictIndex) {
    let cfg = load_openmw_config(config_path);
    let data_paths = cfg.data_directories();
    let archives: Vec<&str> = cfg
        .fallback_archives_iter()
        .map(|a| a.value().as_str())
        .collect();
    VFS::from_directories_with_conflict_index(data_paths, Some(archives))
}

pub fn build_layer_index(config_path: PathBuf) -> (VFS, LayerIndex) {
    let cfg = load_openmw_config(config_path);
    let data_paths = cfg.data_directories();
    let archives: Vec<&str> = cfg
        .fallback_archives_iter()
        .map(|a| a.value().as_str())
        .collect();
    VFS::from_directories_with_layer_index(data_paths, Some(archives))
}

fn validate_config_dir(dir: &PathBuf) -> io::Result<PathBuf> {
    let dir_metadata = std::fs::metadata(dir);

    if dir_metadata.is_ok() && dir_metadata.as_ref().is_ok_and(std::fs::Metadata::is_dir) {
        match fs::read_dir(dir)?
            .filter_map(std::result::Result::ok)
            .find(|entry| entry.file_name().eq_ignore_ascii_case("openmw.cfg"))
            .map(|entry| entry.path())
        {
            Some(cfg) => return Ok(cfg),
            None => {
                eprintln!("[ ERROR ]: No openmw.cfg found in '{}'.", dir.display());
            }
        }
    } else {
        eprintln!(
            "[ ERROR ]: The requested openmw.cfg directory '{}' does not exist or is not a directory.",
            dir.display()
        );
    }

    Err(std::io::Error::new(
        io::ErrorKind::NotFound,
        "Unable to resolve openmw.cfg path.",
    ))
}

pub fn resolve_config_path(config_dir: Option<PathBuf>) -> io::Result<PathBuf> {
    match config_dir {
        Some(dir) => validate_config_dir(&dir),
        None => match std::env::var_os("OPENMW_CONFIG") {
            Some(path) => {
                let path = PathBuf::from(path);
                if path.is_file() {
                    Ok(path)
                } else {
                    eprintln!(
                        "[ ERROR ]: OPENMW_CONFIG '{}' does not exist or is not a file.",
                        path.display()
                    );
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Unable to resolve OPENMW_CONFIG path.",
                    ))
                }
            }
            None => validate_config_dir(&openmw_config::default_config_path()),
        },
    }
}

pub fn construct_vfs(config_path: PathBuf) -> VFS {
    let config = match openmw_config::OpenMWConfiguration::new(Some(config_path)) {
        Err(config_err) => {
            eprintln!("Failed to load configuration file: {config_err}");
            std::process::exit(VFSToolExitCode::FailedToLoadOpenMWConfig.into());
        }
        Ok(config) => config,
    };

    let data_paths = config.data_directories();
    let archives = config
        .fallback_archives_iter()
        .map(|archive| archive.value().as_str())
        .collect();

    VFS::from_directories(data_paths, Some(archives))
}
