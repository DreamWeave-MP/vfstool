// SPDX-License-Identifier: GPL-3.0-only
use std::{
    ffi::OsStr,
    fs,
    io::{self, Result},
    path::{Path, PathBuf},
};

use vfstool_lib::{
    CollapseOptions, VFS, VfsKeyInput, normalize_host_path, run_finalize_tracked, run_setup_tracked,
};

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
    path: &Path,
    format: OutputFormat,
    output: Option<PathBuf>,
    use_relative: bool,
) -> Result<()> {
    let path_string = path.to_string_lossy().to_string();
    let tree = match vfs.find_by_regex(&path_string, use_relative) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(VFSToolExitCode::BadRegex.into());
        }
    };
    write_serialized_vfs(output, format, &tree)
}

fn print_find_file_success(path: &Path, path_display: &str, simple: bool) {
    if simple {
        println!("{path_display}");
    } else {
        println!(
            "{}Successfully found VFS File {} at path {}",
            print::success_prefix(),
            print::blue(path.display()),
            print::green(path_display),
        );
    }
}

fn print_find_file_missing(path: &Path, simple: bool, only_physical: bool) {
    if !simple {
        if only_physical {
            eprintln!(
                "{}Failed to locate {} in loose files of the provided VFS.",
                print::err_prefix(),
                print::blue(path.display()),
            );
        } else {
            eprintln!(
                "{}Failed to locate {} in the provided VFS.",
                print::err_prefix(),
                print::blue(path.display()),
            );
        }
    }
}

fn handle_find_file_fast(resolved_config_dir: PathBuf, path: &Path, simple: bool) -> bool {
    let config = load_openmw_config(resolved_config_dir);
    let data_dirs: Vec<PathBuf> = config
        .data_directories_iter()
        .map(|dir| dir.parsed().to_owned())
        .collect();

    let Some(found) = find_loose_file_fast(data_dirs.iter().rev().map(PathBuf::as_path), path)
    else {
        return false;
    };

    print_find_file_success(path, &found.to_string_lossy(), simple);
    true
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

fn handle_find_file(vfs: &VFS, path: &PathBuf, simple: bool, only_physical: bool) {
    let Some(file) = vfs.get_file(path) else {
        print_find_file_missing(path, simple, false);
        std::process::exit(VFSToolExitCode::FindFailed.into());
    };

    if file.is_archive() && only_physical {
        print_find_file_missing(path, simple, true);
        std::process::exit(VFSToolExitCode::FileNotInLooseDirectories.into());
    }

    let path_display = if file.is_archive() {
        PathBuf::from(file.parent_archive_path().unwrap_or_default())
            .join(path)
            .to_string_lossy()
            .to_string()
    } else {
        file.path().to_string_lossy().to_string()
    };

    print_find_file_success(path, &path_display, simple);
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

    validate_configured_data_path(&config, filter_path);

    let all_dirs: Vec<PathBuf> = config
        .data_directories_iter()
        .map(|dir| dir.parsed().to_owned())
        .collect();

    let tree = vfs.remaining(filter_path, replacements_only, &all_dirs, use_relative);
    write_serialized_vfs(output, format, &tree)
}

fn validate_configured_data_path(config: &openmw_config::OpenMWConfiguration, path: &Path) {
    let normalized = normalize_host_path(path);
    let is_configured = config
        .data_directories_iter()
        .map(openmw_config::DirectorySetting::parsed)
        .any(|dir| normalize_host_path(dir).as_ref() == normalized.as_ref());
    if !is_configured {
        eprintln!(
            "{}{} is not a configured data directory.",
            print::err_prefix(),
            path.display()
        );
        std::process::exit(VFSToolExitCode::InvalidInput.into());
    }
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

fn handle_duplicates(
    vfs: &VFS,
    pattern: Option<&str>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let report = match pattern {
        Some(pattern) => match vfs.duplicates_matching_regex(pattern) {
            Ok(report) => report,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(VFSToolExitCode::BadRegex.into());
            }
        },
        None => vfs.duplicates(),
    };
    write_serialized(output, format, &report)
}

fn handle_archives(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.archives())
}

fn handle_archive_list(
    vfs: &VFS,
    archive: Option<&Path>,
    source_index: Option<usize>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let archive = resolve_archive_selector(vfs, archive, source_index);
    write_serialized(output, format, &vfs.archive_entries(archive))
}

fn normalized_path_text(path: &Path) -> String {
    normalize_host_path(path).to_string_lossy().into_owned()
}

fn archive_selector_matches(selector: &Path, archive_path: &Path) -> bool {
    let selector_text = normalized_path_text(selector);
    let archive_text = normalized_path_text(archive_path);

    if selector_text == archive_text {
        return true;
    }

    archive_path
        .file_name()
        .is_some_and(|name| normalize_host_path(Path::new(name)) == Path::new(&selector_text))
        || archive_text
            .strip_suffix(&selector_text)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
}

fn resolve_archive_selector(
    vfs: &VFS,
    archive: Option<&Path>,
    source_index: Option<usize>,
) -> PathBuf {
    let archives = vfs.archives();

    if let Some(source_index) = source_index {
        return archives
            .iter()
            .find(|info| info.source_index == source_index)
            .map_or_else(|| {
                eprintln!(
                    "{}No loaded archive has source index {source_index}. Available archives: {}",
                    print::err_prefix(),
                    format_archive_choices(&archives)
                );
                std::process::exit(VFSToolExitCode::InvalidInput.into());
            }, |info| info.path.clone());
    }

    let Some(selector) = archive else {
        eprintln!(
            "{}archive-list needs an archive selector or --source-index. Available archives: {}",
            print::err_prefix(),
            format_archive_choices(&archives)
        );
        std::process::exit(VFSToolExitCode::InvalidInput.into());
    };

    let matches: Vec<_> = archives
        .iter()
        .filter(|info| archive_selector_matches(selector, &info.path))
        .collect();

    match matches.as_slice() {
        [info] => info.path.clone(),
        [] => {
            eprintln!(
                "{}No loaded archive matches '{}'. Available archives: {}",
                print::err_prefix(),
                selector.display(),
                format_archive_choices(&archives)
            );
            std::process::exit(VFSToolExitCode::InvalidInput.into());
        }
        _ => {
            eprintln!(
                "{}Archive selector '{}' is ambiguous. Use --source-index or a longer path suffix. Matches: {}",
                print::err_prefix(),
                selector.display(),
                format_archive_choices(matches.iter().copied())
            );
            std::process::exit(VFSToolExitCode::InvalidInput.into());
        }
    }
}

fn format_archive_choices<'a>(
    archives: impl IntoIterator<Item = &'a vfstool_lib::ArchiveInfo>,
) -> String {
    let choices: Vec<_> = archives
        .into_iter()
        .map(|info| format!("{}={}", info.source_index, info.path.display()))
        .collect();
    if choices.is_empty() {
        "<none>".to_owned()
    } else {
        choices.join(", ")
    }
}

fn handle_contributions(vfs: &VFS, format: OutputFormat, output: Option<PathBuf>) -> Result<()> {
    write_serialized(output, format, &vfs.source_contributions())
}

#[derive(vfstool_lib::serde::Serialize)]
#[serde(crate = "vfstool_lib::serde")]
struct AppValidationReport {
    issues: Vec<AppValidationIssue>,
}

#[derive(vfstool_lib::serde::Serialize)]
#[serde(crate = "vfstool_lib::serde")]
enum AppValidationIssue {
    MissingDataDirectory { path: PathBuf },
    DataPathNotDirectory { path: PathBuf },
    MissingFallbackArchive { name: String },
    UnreadableFallbackArchive { name: String, path: PathBuf },
    MissingContentFile { name: String },
    MissingGroundcoverFile { name: String },
}

fn handle_validate_fast(
    resolved_config_dir: PathBuf,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let cfg = load_openmw_config(resolved_config_dir);
    let data_dirs: Vec<PathBuf> = cfg
        .data_directories_iter()
        .map(|dir| dir.parsed().to_owned())
        .collect();
    let mut issues = Vec::new();

    for path in &data_dirs {
        if !path.exists() {
            issues.push(AppValidationIssue::MissingDataDirectory { path: path.clone() });
        } else if !path.is_dir() {
            issues.push(AppValidationIssue::DataPathNotDirectory { path: path.clone() });
        }
    }

    for archive in cfg.fallback_archives_iter() {
        let name = archive.value();
        if find_loose_file_fast(
            data_dirs.iter().rev().map(PathBuf::as_path),
            Path::new(name),
        )
        .is_none()
        {
            issues.push(AppValidationIssue::MissingFallbackArchive { name: name.clone() });
        }
    }

    for content in cfg.content_files_iter() {
        let name = content.value();
        if find_loose_file_fast(
            data_dirs.iter().rev().map(PathBuf::as_path),
            Path::new(name),
        )
        .is_none()
        {
            issues.push(AppValidationIssue::MissingContentFile { name: name.clone() });
        }
    }

    for groundcover in cfg.groundcover_iter() {
        let name = groundcover.value();
        if find_loose_file_fast(
            data_dirs.iter().rev().map(PathBuf::as_path),
            Path::new(name),
        )
        .is_none()
        {
            issues.push(AppValidationIssue::MissingGroundcoverFile { name: name.clone() });
        }
    }

    write_serialized(output, format, &AppValidationReport { issues })
}

fn handle_validate_full(
    resolved_config_dir: PathBuf,
    vfs: &VFS,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let cfg = load_openmw_config(resolved_config_dir);
    let mut issues = Vec::new();

    for dir in cfg.data_directories_iter() {
        let path = dir.parsed();
        if !path.exists() {
            issues.push(AppValidationIssue::MissingDataDirectory {
                path: path.to_path_buf(),
            });
        } else if !path.is_dir() {
            issues.push(AppValidationIssue::DataPathNotDirectory {
                path: path.to_path_buf(),
            });
        }
    }

    let loaded_archives = vfs.archives();
    for archive in cfg.fallback_archives_iter() {
        let name = archive.value();
        let Some(file) = vfs.get_file(name.as_str()) else {
            issues.push(AppValidationIssue::MissingFallbackArchive { name: name.clone() });
            continue;
        };
        let path = file.path().to_path_buf();
        if !loaded_archives.iter().any(|info| info.path == path) {
            issues.push(AppValidationIssue::UnreadableFallbackArchive {
                name: name.clone(),
                path,
            });
        }
    }

    for content in cfg.content_files_iter() {
        let name = content.value();
        if vfs.get_file(name.as_str()).is_none() {
            issues.push(AppValidationIssue::MissingContentFile { name: name.clone() });
        }
    }

    for groundcover in cfg.groundcover_iter() {
        let name = groundcover.value();
        if vfs.get_file(name.as_str()).is_none() {
            issues.push(AppValidationIssue::MissingGroundcoverFile { name: name.clone() });
        }
    }

    write_serialized(output, format, &AppValidationReport { issues })
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
        Commands::Duplicates {
            pattern,
            format,
            output,
        } => {
            handle_duplicates(vfs, pattern.as_deref(), format, output)?;
            Ok(None)
        }
        Commands::Archives { format, output } => {
            handle_archives(vfs, format, output)?;
            Ok(None)
        }
        Commands::ArchiveList {
            archive,
            source_index,
            format,
            output,
        } => {
            handle_archive_list(vfs, archive.as_deref(), source_index, format, output)?;
            Ok(None)
        }
        Commands::Contributions { format, output } => {
            handle_contributions(vfs, format, output)?;
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
            dir.parsed().to_path_buf()
        } else {
            eprintln!(
                "{}No data-local set in openmw.cfg; use --output to specify a destination.",
                print::err_prefix()
            );
            std::process::exit(VFSToolExitCode::InvalidInput.into());
        }
    });

    let merged = params.merged_dir;
    if merged
        .read_dir()
        .is_ok_and(|mut entries| entries.next().is_some())
    {
        eprintln!(
            "{}merged directory {} already exists and is not empty; choose an empty scratch directory.",
            print::err_prefix(),
            merged.display()
        );
        std::process::exit(VFSToolExitCode::InvalidInput.into());
    }
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

    let cleanup_result = if params.keep_merged {
        Ok(())
    } else {
        fs::remove_dir_all(&merged).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("failed to remove merged dir '{}': {e}", merged.display()),
            )
        })
    };

    inner_result?;
    if let Err(e) = cleanup_result {
        if subprocess_status.is_some_and(|status| !status.success()) {
            eprintln!("vfstool: {e}");
        } else {
            return Err(e);
        }
    }
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
    list_files: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (_, ci) = build_conflict_index(resolved_config_dir);
    let report = ci.shadowed_report_with_files(use_relative, list_files);
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
    let cfg = load_openmw_config(resolved_config_dir.clone());
    validate_configured_data_path(&cfg, source_a);
    validate_configured_data_path(&cfg, source_b);
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
            handle_find(vfs, path.as_path(), format, output, use_relative)?;
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
        Commands::Validate {
            full: true,
            format,
            output,
        } => {
            handle_validate_full(resolved_config_dir, vfs, format, output)?;
            Ok(None)
        }
        Commands::Validate { full: false, .. } => Ok(Some(command)),
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
        Commands::Shadowed {
            list_files,
            format,
            output,
        } => {
            handle_shadowed(
                resolved_config_dir,
                use_relative,
                list_files,
                format,
                output,
            )?;
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
    if let Commands::FindFile { path, simple, .. } = &command {
        if handle_find_file_fast(resolved_config_dir.clone(), path, *simple) {
            return Ok(());
        }
    }

    if let Commands::Validate {
        full: false,
        format,
        output,
    } = command
    {
        return handle_validate_fast(resolved_config_dir, format, output);
    }

    let needs_plain_vfs = matches!(
        command,
        Commands::Collapse { .. }
            | Commands::Extract { .. }
            | Commands::Find { .. }
            | Commands::FindFile { .. }
            | Commands::Remaining { .. }
            | Commands::Run { .. }
            | Commands::Explain { .. }
            | Commands::Duplicates { .. }
            | Commands::Archives { .. }
            | Commands::ArchiveList { .. }
            | Commands::Contributions { .. }
            | Commands::Validate { full: true, .. }
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
