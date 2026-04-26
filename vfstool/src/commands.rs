// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{
    fs,
    io::Result,
    path::{Path, PathBuf},
};

use vfstool_lib::{CollapseOptions, VFS, normalize_path, run_finalize_tracked, run_setup_tracked};

use crate::{
    cli::{Commands, OutputFormat},
    config::{build_conflict_index, build_layer_index, construct_vfs, load_openmw_config},
    exit::VFSToolExitCode,
    output::{parse_lock_file, write_serialized, write_serialized_vfs},
    print,
};

struct CollapseParams {
    collapse_into: PathBuf,
    options: CollapseOptions,
    dry_run: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
}

fn handle_collapse(vfs: &VFS, params: CollapseParams) -> Result<()> {
    if params.dry_run {
        let plan = vfs.materialization_plan(&params.collapse_into, &params.options);
        write_serialized(params.output, params.format, &plan)
    } else {
        vfs.collapse_into(&params.collapse_into, &params.options)
    }
}

fn handle_extract(vfs: &VFS, source_file: &Path, target_dir: &Path) -> Result<()> {
    match vfs.extract_file(source_file, target_dir)? {
        None => {
            eprintln!(
                "{}Couldn't locate {} in the vfs!",
                print::err_prefix(),
                print::green(source_file.display()),
            );
            std::process::exit(VFSToolExitCode::FindFailed.into());
        }
        Some(dest) => println!(
            "{}Successfully extracted {} to {}",
            print::success_prefix(),
            print::green(source_file.display()),
            print::blue(dest.parent().unwrap_or(target_dir).display()),
        ),
    }
    Ok(())
}

fn handle_find(
    vfs: &VFS,
    path: &PathBuf,
    format: OutputFormat,
    output: Option<PathBuf>,
    use_relative: bool,
) -> Result<()> {
    let path_string = normalize_path(&path).to_string_lossy().to_string();
    let tree = match vfs.find_by_regex(&path_string, use_relative) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(VFSToolExitCode::BadRegex.into());
        }
    };
    write_serialized_vfs(output, format, &tree)
}

fn handle_find_file(vfs: &VFS, path: &PathBuf, simple: bool, only_physical: bool) {
    let Some(file) = vfs.get_file(path) else {
        if !simple {
            eprintln!(
                "{}Failed to locate {} in the provided VFS.",
                print::err_prefix(),
                print::blue(path.display()),
            );
        }
        std::process::exit(VFSToolExitCode::FindFailed.into());
    };

    let path_display = if file.is_archive() {
        if only_physical {
            if !simple {
                eprintln!(
                    "{}Failed to locate {} in loose files of the provided VFS.",
                    print::err_prefix(),
                    print::blue(path.display()),
                );
            }
            std::process::exit(VFSToolExitCode::FileNotInLooseDirectories.into());
        }
        PathBuf::from(file.parent_archive_path().unwrap_or_default())
            .join(path)
            .to_string_lossy()
            .to_string()
    } else {
        file.path().to_string_lossy().to_string()
    };

    if simple {
        println!("{path_display}");
    } else {
        println!(
            "{}Successfully found VFS File {} at path {}",
            print::success_prefix(),
            print::blue(path.display()),
            print::green(&path_display),
        );
    }
}

fn handle_remaining(
    vfs: &VFS,
    resolved_config_dir: PathBuf,
    filter_path: &Path,
    replacements_only: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
    use_relative: bool,
) -> Result<()> {
    let config = match openmw_config::OpenMWConfiguration::new(Some(resolved_config_dir)) {
        Err(config_err) => {
            eprintln!("Failed to load openmw.cfg for comparison: {config_err}");
            std::process::exit(VFSToolExitCode::FailedToLoadOpenMWConfig.into());
        }
        Ok(config) => config,
    };

    let all_dirs: Vec<PathBuf> = config
        .data_directories_iter()
        .map(|dir| dir.parsed().to_owned())
        .collect();

    let tree = vfs.remaining(filter_path, replacements_only, &all_dirs, use_relative);
    write_serialized_vfs(output, format, &tree)
}

fn handle_which(
    vfs: &VFS,
    path: &PathBuf,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let normalized = normalize_path(&path).into_owned();
    let Some(result) = vfs.explain(&normalized) else {
        eprintln!(
            "{}VFS path '{}' not found.",
            print::err_prefix(),
            path.display()
        );
        std::process::exit(VFSToolExitCode::FindFailed.into());
    };
    write_serialized(output, format, &result)
}

fn handle_explain(
    vfs: &VFS,
    path: &Path,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let Some(report) = vfs.explain(path) else {
        eprintln!(
            "{}VFS path '{}' not found.",
            print::err_prefix(),
            path.display()
        );
        std::process::exit(VFSToolExitCode::FindFailed.into());
    };
    write_serialized(output, format, &report)
}

fn handle_duplicates(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.duplicates())
}

fn handle_archives(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.archives())
}

fn handle_archive_list(
    vfs: &VFS,
    archive: &Path,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    write_serialized(output, format, &vfs.archive_entries(archive))
}

fn handle_case_collisions(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.case_collisions())
}

fn handle_contributions(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.source_contributions())
}

fn handle_validate(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.validate())
}

fn run_provider_vfs_command(command: Commands, vfs: &VFS) -> Result<Option<Commands>> {
    match command {
        Commands::Explain {
            path,
            format,
            output,
        } => {
            handle_explain(vfs, path.as_path(), format, output)?;
            Ok(None)
        }
        Commands::Duplicates { format, output } => {
            handle_duplicates(vfs, format, output)?;
            Ok(None)
        }
        Commands::Archives { format, output } => {
            handle_archives(vfs, format, output)?;
            Ok(None)
        }
        Commands::ArchiveList {
            archive,
            format,
            output,
        } => {
            handle_archive_list(vfs, archive.as_path(), format, output)?;
            Ok(None)
        }
        Commands::CaseCollisions { format, output } => {
            handle_case_collisions(vfs, format, output)?;
            Ok(None)
        }
        Commands::Contributions { format, output } => {
            handle_contributions(vfs, format, output)?;
            Ok(None)
        }
        Commands::Validate { format, output } => {
            handle_validate(vfs, format, output)?;
            Ok(None)
        }
        Commands::Which {
            path,
            format,
            output,
        } => {
            handle_which(vfs, &path, format, output)?;
            Ok(None)
        }
        other => Ok(Some(other)),
    }
}

pub struct RunParams<'a> {
    merged_dir: PathBuf,
    command: &'a [String],
    keep_merged: bool,
    output: Option<PathBuf>,
    copy: bool,
    working_dir: &'a Option<PathBuf>,
}

fn handle_run(vfs: &VFS, resolved_config_dir: PathBuf, params: RunParams<'_>) -> Result<()> {
    let cfg = load_openmw_config(resolved_config_dir);
    let data_local: PathBuf = params.output.unwrap_or_else(|| {
        if let Some(dir) = cfg.data_local() {
            dir.parsed().clone()
        } else {
            eprintln!(
                "{}No data-local set in openmw.cfg; use --output to specify a destination.",
                print::err_prefix()
            );
            std::process::exit(VFSToolExitCode::InvalidInput.into());
        }
    });

    let merged = params.merged_dir;
    let (inner_result, subprocess_status) =
        (|| -> (Result<()>, Option<std::process::ExitStatus>) {
            eprintln!("Dumping VFS to {}...", merged.display());
            let (count, baseline) = match run_setup_tracked(vfs, &merged, !params.copy) {
                Ok(r) => r,
                Err(e) => return (Err(e), None),
            };
            eprintln!("Dumped {count} files.");

            let substituted: Vec<String> = params
                .command
                .iter()
                .map(|arg| {
                    if arg == "{}" {
                        merged.to_string_lossy().into_owned()
                    } else {
                        arg.clone()
                    }
                })
                .collect();

            let mut cmd = std::process::Command::new(&substituted[0]);
            cmd.args(&substituted[1..]);
            if let Some(dir) = params.working_dir {
                cmd.current_dir(dir);
            }
            let status = match cmd.status() {
                Ok(s) => s,
                Err(e) => return (Err(e), None),
            };

            if !status.success() {
                eprintln!("vfstool: subprocess exited with {status}, not capturing files.");
                return (Ok(()), Some(status));
            }

            let copied = match run_finalize_tracked(&merged, &baseline, &data_local) {
                Ok(c) => c,
                Err(e) => return (Err(e), Some(status)),
            };

            if copied.is_empty() {
                eprintln!("No files changed.");
            } else {
                eprintln!(
                    "Capturing {} changed file(s) to {}...",
                    copied.len(),
                    data_local.display()
                );
                for (rel, dest) in &copied {
                    println!("{} -> {}", rel.display(), dest.display());
                }
            }
            (Ok(()), Some(status))
        })();

    if !params.keep_merged {
        let _ = fs::remove_dir_all(&merged);
    }
    inner_result?;
    std::process::exit(
        subprocess_status
            .and_then(|s| s.code())
            .unwrap_or(VFSToolExitCode::RuntimeFailure.into()),
    );
}

fn handle_conflicts(
    resolved_config_dir: PathBuf,
    use_relative: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (_, ci) = build_conflict_index(resolved_config_dir);
    let report = ci.conflicts_report(use_relative);
    write_serialized(output, format, &report)
}

fn handle_shadowed(
    resolved_config_dir: PathBuf,
    use_relative: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (_, ci) = build_conflict_index(resolved_config_dir);
    let report = ci.shadowed_report(use_relative);
    eprintln!("{} sources are fully shadowed", report.sources.len());
    write_serialized(output, format, &report)
}

fn handle_diff(
    resolved_config_dir: PathBuf,
    source_a: &Path,
    source_b: &Path,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (_, ci) = build_conflict_index(resolved_config_dir);
    let report = ci.diff_report(source_a, source_b);
    write_serialized(output, format, &report)
}

fn handle_lock(
    resolved_config_dir: PathBuf,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let lock = layer.lock_manifest(&vfs)?;
    write_serialized(output, format, &lock)
}

fn handle_drift(
    resolved_config_dir: PathBuf,
    lock_file: &Path,
    fail_on_drift: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let lock = parse_lock_file(lock_file)?;
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let report = layer.diff_against_lock(&vfs, &lock)?;
    let has_drift = !report.entries.is_empty();
    write_serialized(output, format, &report)?;
    if has_drift && fail_on_drift {
        std::process::exit(VFSToolExitCode::DriftDetected.into());
    }
    Ok(())
}

fn run_core_vfs_command(
    command: Commands,
    vfs: &VFS,
    use_relative: bool,
    resolved_config_dir: PathBuf,
) -> Result<Option<Commands>> {
    match command {
        Commands::Collapse {
            collapse_into,
            allow_copying,
            extract_archives,
            symbolic,
            dry_run,
            format,
            output,
        } => {
            handle_collapse(
                vfs,
                CollapseParams {
                    collapse_into,
                    options: CollapseOptions {
                        allow_copying,
                        extract_archives,
                        use_symlinks: symbolic,
                    },
                    dry_run,
                    format,
                    output,
                },
            )?;
            Ok(None)
        }
        Commands::Extract {
            source_file,
            target_dir,
        } => {
            handle_extract(vfs, source_file.as_path(), target_dir.as_path())?;
            Ok(None)
        }
        Commands::Find {
            path,
            format,
            output,
        } => {
            handle_find(vfs, &path, format, output, use_relative)?;
            Ok(None)
        }
        Commands::FindFile {
            path,
            simple,
            only_physical,
        } => {
            handle_find_file(vfs, &path, simple, only_physical);
            Ok(None)
        }
        Commands::Remaining {
            filter_path,
            replacements_only,
            format,
            output,
        } => {
            handle_remaining(
                vfs,
                resolved_config_dir,
                filter_path.as_path(),
                replacements_only,
                format,
                output,
                use_relative,
            )?;
            Ok(None)
        }
        Commands::Run {
            merged_dir,
            command,
            keep_merged,
            output,
            copy,
            working_dir,
        } => {
            handle_run(
                vfs,
                resolved_config_dir,
                RunParams {
                    merged_dir,
                    command: &command,
                    keep_merged,
                    output,
                    copy,
                    working_dir: &working_dir,
                },
            )?;
            Ok(None)
        }
        other => run_provider_vfs_command(other, vfs),
    }
}

fn run_analysis_primary(
    command: Commands,
    use_relative: bool,
    resolved_config_dir: PathBuf,
) -> Result<Option<Commands>> {
    match command {
        Commands::Conflicts { format, output } => {
            handle_conflicts(resolved_config_dir, use_relative, format, output)?;
            Ok(None)
        }
        Commands::Shadowed { format, output } => {
            handle_shadowed(resolved_config_dir, use_relative, format, output)?;
            Ok(None)
        }
        Commands::Diff {
            source_a,
            source_b,
            format,
            output,
        } => handle_diff(
            resolved_config_dir,
            source_a.as_path(),
            source_b.as_path(),
            format,
            output,
        )
        .map(|()| None),
        Commands::Lock { format, output } => {
            handle_lock(resolved_config_dir, format, output).map(|()| None)
        }
        Commands::Drift {
            lock_file,
            fail_on_drift,
            format,
            output,
        } => handle_drift(
            resolved_config_dir,
            lock_file.as_path(),
            fail_on_drift,
            format,
            output,
        )
        .map(|()| None),
        other => Ok(Some(other)),
    }
}

fn run_analysis_command(
    command: Commands,
    use_relative: bool,
    resolved_config_dir: PathBuf,
) -> Result<()> {
    if run_analysis_primary(command, use_relative, resolved_config_dir)?.is_none() {
        return Ok(());
    }
    unreachable!("all 1.0 commands should be handled before this point")
}

pub fn run_command(
    command: Commands,
    use_relative: bool,
    resolved_config_dir: PathBuf,
) -> Result<()> {
    let needs_plain_vfs = matches!(
        command,
        Commands::Collapse { .. }
            | Commands::Extract { .. }
            | Commands::Find { .. }
            | Commands::FindFile { .. }
            | Commands::Remaining { .. }
            | Commands::Run { .. }
            | Commands::Which { .. }
            | Commands::Explain { .. }
            | Commands::Duplicates { .. }
            | Commands::Archives { .. }
            | Commands::ArchiveList { .. }
            | Commands::CaseCollisions { .. }
            | Commands::Contributions { .. }
            | Commands::Validate { .. }
    );

    if !needs_plain_vfs {
        return run_analysis_command(command, use_relative, resolved_config_dir);
    }

    let vfs = construct_vfs(resolved_config_dir.clone());
    let Some(command) =
        run_core_vfs_command(command, &vfs, use_relative, resolved_config_dir.clone())?
    else {
        return Ok(());
    };

    run_analysis_command(command, use_relative, resolved_config_dir)
}
