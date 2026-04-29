// SPDX-License-Identifier: GPL-3.0-only
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use vfstool_lib::{VFS, VfsKeyInput};

struct ValidateInputs {
    config_path: PathBuf,
    data_dirs: Vec<PathBuf>,
    archives: Vec<String>,
    content_files: Vec<String>,
    groundcover_files: Vec<String>,
}

fn resolve_default_openmw_cfg() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("OPENMW_CONFIG").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "OPENMW_CONFIG '{}' does not exist or is not a file",
                path.display()
            ),
        ));
    }

    let dir = openmw_config::default_config_path();
    fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().eq_ignore_ascii_case("openmw.cfg"))
        .map(|entry| entry.path())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("No openmw.cfg found in '{}'", dir.display()),
            )
        })
}

fn load_inputs() -> io::Result<ValidateInputs> {
    let config_path = resolve_default_openmw_cfg()?;
    let cfg = openmw_config::OpenMWConfiguration::new(Some(config_path.clone()))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    Ok(ValidateInputs {
        config_path,
        data_dirs: cfg
            .data_directories_iter()
            .map(|dir| dir.parsed().to_owned())
            .collect(),
        archives: cfg
            .fallback_archives_iter()
            .map(|archive| archive.value().clone())
            .collect(),
        content_files: cfg
            .content_files_iter()
            .map(|content| content.value().clone())
            .collect(),
        groundcover_files: cfg
            .groundcover_iter()
            .map(|groundcover| groundcover.value().clone())
            .collect(),
    })
}

fn archive_refs(archives: &[String]) -> Vec<&str> {
    archives.iter().map(String::as_str).collect()
}

fn build_loose_only_vfs(inputs: &ValidateInputs) -> VFS {
    VFS::from_directories(inputs.data_dirs.iter().map(PathBuf::as_path), None)
}

fn build_full_vfs(inputs: &ValidateInputs) -> VFS {
    VFS::from_directories(
        inputs.data_dirs.iter().map(PathBuf::as_path),
        Some(archive_refs(&inputs.archives)),
    )
}

fn run_fast_reference_checks(inputs: &ValidateInputs) -> usize {
    let data_dirs = inputs.data_dirs.iter().rev().map(PathBuf::as_path);
    let mut missing = 0;

    for archive in &inputs.archives {
        if find_loose_file_fast(data_dirs.clone(), Path::new(archive)).is_none() {
            missing += 1;
        }
    }

    for content in &inputs.content_files {
        if find_loose_file_fast(data_dirs.clone(), Path::new(content)).is_none() {
            missing += 1;
        }
    }

    for groundcover in &inputs.groundcover_files {
        if find_loose_file_fast(data_dirs.clone(), Path::new(groundcover)).is_none() {
            missing += 1;
        }
    }

    missing
}

fn find_loose_file_fast<'a>(
    mut data_dirs: impl Iterator<Item = &'a Path>,
    path: &Path,
) -> Option<PathBuf> {
    let key = path.to_safe_vfs_key()?;
    let components: Vec<&[u8]> = key
        .as_bytes()
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty())
        .collect();

    if components.is_empty() {
        return None;
    }

    data_dirs.find_map(|data_dir| resolve_case_insensitive_file(data_dir, &components))
}

fn resolve_case_insensitive_file(data_dir: &Path, components: &[&[u8]]) -> Option<PathBuf> {
    let mut candidates = vec![data_dir.to_path_buf()];

    for (index, component) in components.iter().enumerate() {
        let is_last = index == components.len() - 1;
        let mut matches = Vec::new();

        for candidate in &candidates {
            let Ok(entries) = fs::read_dir(candidate) else {
                continue;
            };

            for entry in entries.flatten() {
                if normalized_os_component(&entry.file_name()) != *component {
                    continue;
                }

                let path = entry.path();
                if is_last {
                    if path.is_file() {
                        matches.push(path);
                    }
                } else if path.is_dir() {
                    matches.push(path);
                }
            }
        }

        if matches.is_empty() {
            return None;
        }

        candidates = matches;
    }

    candidates.into_iter().max()
}

fn normalized_os_component(component: &OsStr) -> Vec<u8> {
    component
        .as_encoded_bytes()
        .iter()
        .map(|byte| {
            if *byte == b'\\' {
                b'/'
            } else {
                byte.to_ascii_lowercase()
            }
        })
        .collect()
}

fn time_phase<T>(name: &str, run: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let output = run();
    let elapsed = start.elapsed();
    eprintln!("  {name:<32} {elapsed:?}");
    (output, elapsed)
}

fn print_duration_delta(name: &str, left: Duration, right: Duration) {
    let left_nanos = left.as_nanos();
    let right_nanos = right.as_nanos();
    let magnitude = left_nanos.abs_diff(right_nanos);
    let sign = if left_nanos >= right_nanos { "+" } else { "-" };
    eprintln!("  {name:<32} {sign}{magnitude}ns");
}

fn print_single_run_profile() -> io::Result<()> {
    eprintln!("Single-run validate-chain phase profile:");
    let (config_path, _) = time_phase("resolve_default_config_path", resolve_default_openmw_cfg);
    let config_path = config_path?;
    let (cfg, _) = time_phase("load_openmw_config", || {
        openmw_config::OpenMWConfiguration::new(Some(config_path.clone()))
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    });
    let cfg = cfg?;

    let (inputs, _) = time_phase("collect_config_inputs", || ValidateInputs {
        config_path,
        data_dirs: cfg
            .data_directories_iter()
            .map(|dir| dir.parsed().to_owned())
            .collect(),
        archives: cfg
            .fallback_archives_iter()
            .map(|archive| archive.value().clone())
            .collect(),
        content_files: cfg
            .content_files_iter()
            .map(|content| content.value().clone())
            .collect(),
        groundcover_files: cfg
            .groundcover_iter()
            .map(|groundcover| groundcover.value().clone())
            .collect(),
    });

    let (_, _) = time_phase("validate_fast_references", || {
        run_fast_reference_checks(&inputs)
    });
    let (_, loose_elapsed) =
        time_phase("construct_loose_only_vfs", || build_loose_only_vfs(&inputs));
    let (full_vfs, full_elapsed) = time_phase("construct_full_vfs", || build_full_vfs(&inputs));
    print_duration_delta("construct_full_minus_loose", full_elapsed, loose_elapsed);
    let (_, _) = time_phase("validate_winners", || full_vfs.validate_winners());
    let (_, _) = time_phase("validate_full", || full_vfs.validate());

    Ok(())
}

fn bench_real_validate_chain(c: &mut Criterion) {
    let Ok(inputs) = load_inputs() else {
        eprintln!(
            "Skipping real_openmw_validate_chain: no default openmw.cfg found (or OPENMW_CONFIG is invalid)"
        );
        return;
    };

    if let Err(err) = print_single_run_profile() {
        eprintln!("Unable to print validate-chain phase profile: {err}");
    }

    eprintln!(
        "Benchmarking validate chain for '{}' ({} data dirs, {} fallback archives, {} content files, {} groundcover files)",
        inputs.config_path.display(),
        inputs.data_dirs.len(),
        inputs.archives.len(),
        inputs.content_files.len(),
        inputs.groundcover_files.len()
    );

    let mut group = c.benchmark_group("real_openmw_validate_chain");
    group.sample_size(10);

    group.bench_function("resolve_default_config_path", |bench| {
        bench.iter(resolve_default_openmw_cfg);
    });

    group.bench_function("load_openmw_config", |bench| {
        let config_path = inputs.config_path.clone();
        bench.iter(|| openmw_config::OpenMWConfiguration::new(Some(config_path.clone())));
    });

    group.bench_function("collect_loaded_config_inputs", |bench| {
        let cfg = openmw_config::OpenMWConfiguration::new(Some(inputs.config_path.clone()))
            .expect("benchmark config should have loaded during setup");
        bench.iter(|| {
            (
                cfg.data_directories_iter()
                    .map(|dir| dir.parsed().to_owned())
                    .collect::<Vec<_>>(),
                cfg.fallback_archives_iter()
                    .map(|archive| archive.value().clone())
                    .collect::<Vec<_>>(),
                cfg.content_files_iter()
                    .map(|content| content.value().clone())
                    .collect::<Vec<_>>(),
                cfg.groundcover_iter()
                    .map(|groundcover| groundcover.value().clone())
                    .collect::<Vec<_>>(),
            )
        });
    });

    group.bench_function("validate_fast_reference_checks", |bench| {
        bench.iter(|| run_fast_reference_checks(&inputs));
    });

    group.bench_function("construct_loose_only_vfs", |bench| {
        bench.iter(|| build_loose_only_vfs(&inputs));
    });

    group.bench_function("construct_full_vfs_with_archives", |bench| {
        bench.iter(|| build_full_vfs(&inputs));
    });

    let full_vfs_for_winner_validation = build_full_vfs(&inputs);
    group.bench_function("validate_winners_after_full_build", |bench| {
        bench.iter(|| full_vfs_for_winner_validation.validate_winners());
    });

    let full_vfs_for_full_validation = build_full_vfs(&inputs);
    group.bench_function("validate_full_after_full_build", |bench| {
        bench.iter(|| full_vfs_for_full_validation.validate());
    });

    group.finish();
}

criterion_group!(benches, bench_real_validate_chain);
criterion_main!(benches);
