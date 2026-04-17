// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fs,
    io::{self, Result, Write},
    path::PathBuf,
};
use vfstool_lib::{
    CollapseOptions, ConflictIndex, SerializeType, normalize_path, run_finalize, run_setup,
    serialize_value, vfs::VFS,
};

pub enum VFSToolExitCode {
    FindFailed = 1,
    FileNotInLooseDirectories = 2,
    BadRegex = 254,
    FailedToLoadOpenMWConfig = 255,
}

impl From<VFSToolExitCode> for i32 {
    fn from(value: VFSToolExitCode) -> Self {
        match value {
            VFSToolExitCode::FindFailed => 1,
            VFSToolExitCode::FileNotInLooseDirectories => 2,
            VFSToolExitCode::BadRegex => 254,
            VFSToolExitCode::FailedToLoadOpenMWConfig => 255,
        }
    }
}

mod print {
    const GREEN: &str = "\x1b[32m";
    const BLUE: &str = "\x1b[34m";
    const RESET: &str = "\x1b[0m";

    pub const fn err_prefix() -> &'static str {
        concat!("\x1b[31m", "[ ERROR ]", "\x1b[0m", ": ")
    }

    pub const fn success_prefix() -> &'static str {
        concat!("\x1b[32m", "[ SUCCESS ]", "\x1b[0m", ": ")
    }

    pub fn blue<S: std::fmt::Display>(input: S) -> String {
        format!("{BLUE}{input}{RESET}")
    }

    pub fn green<S: std::fmt::Display>(input: S) -> String {
        format!("{GREEN}{input}{RESET}")
    }
}

#[derive(Parser)]
#[command(
    name = "vfstool",
    about = "vfstool allows users to reconstruct and interact with OpenMW's virtual file system in any way they might see fit, using this application to locate files, serialize their VFS to most major text formats, extract files out of the vfs, and even collapse their VFS to a single directory for space savings."
)]
struct Cli {
    /// Path to openmw.cfg.
    ///
    /// Note this is the directory containing it, not the path to the file itself.
    ///
    /// Example: C:\Documents\My Games\openmw
    ///
    /// This argument assumes the config used is called `openmw.cfg`
    /// (case-insensitive).
    ///
    /// If you need to use an openmw.cfg which is named something else,
    ///
    /// set the `OPENMW_CONFIG` variable to the absolute path of your desired config file instead.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Whether or not to use relative paths in output
    #[arg(short = 'r', long)]
    use_relative: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Subcommands for `vfstool`
#[derive(Subcommand)]
enum Commands {
    /// Given a target directory, create a set of hardlinks for the entire virtual
    /// filesystem inside of it. Skyrim support ;)
    Collapse {
        /// Target folder to collapse the VFS into
        collapse_into: PathBuf,

        /// If this is used, any case where hard linking failed or won't work (files in BSA
        /// archives), falls back to normal copying operations
        #[arg(short, long)]
        allow_copying: bool,

        /// If enabled, allows extracting files out of BSA/BA2 archives during collapsing
        #[arg(short, long)]
        extract_archives: bool,

        /// Use symbolic instead of hardlinks, to allow cross-device links
        #[arg(short, long)]
        symbolic: bool,
    },
    /// Extract a given file from the VFS into a given directory
    Extract {
        /// Full relative path to a VFS file, eg meshes/xbase_anim.nif
        source_file: PathBuf,

        /// Directory to extract the file to
        target_dir: PathBuf,
    },
    /// Given some VFS path, like `meshes/xbase_anim.nif`, return its absolute path (if found)
    FindFile {
        /// Full (relative) VFS Path to query.
        /// Returns the absolute path, of the file referenced by this VFS path. EG:
        ///
        /// vfstool find-file meshes/xbase_anim.nif
        ///
        /// C:\Games\Morrowind\Data Files\Meshes\XBase_Anim.nif
        path: PathBuf,

        /// If set, only matches files which are NOT inside an archive (BSA/BA2).
        /// Exits with code 2 if the file exists but is archived.
        #[arg(short = 'p', long = "only_physical")]
        only_physical: bool,

        /// Simple output, no coloration or formatting. Useful for pipes
        #[arg(short, long)]
        simple: bool,
    },
    /// Given some query term, locate all matches in the vfs.
    Find {
        /// VFS Path to query. Supports regular expressions!
        path: PathBuf,

        /// Output format when serializing as text.
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,

        /// Path to save the resulting search tree to.
        ///
        /// If omitted, the result is printed directly to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Given an absolute path, return a filtered version of the VFS containing either things
    /// replacing it, or files from this directory which are not being replaced
    Remaining {
        filter_path: PathBuf,

        /// If used, show only files replacing contents of this path, instead of ones still in it
        #[arg(short, long)]
        replacements_only: bool,

        /// Output format when serializing as text.
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,

        /// Path to save the resulting search tree to.
        ///
        /// If omitted, the result is printed directly to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Analyse conflict relationships across all sources in the load order
    Conflicts {
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Show files that are overridden by higher-priority sources
    Shadowed {
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Given a VFS path, show which source wins and which others also have it
    Which {
        /// Relative VFS path to query, e.g. textures/tx_bc_mudcrab.dds
        path: PathBuf,
    },
    /// Per-source statistics: wins, overrides, overridden file counts
    Stats,
    /// Compare files between two specific data directories
    Diff {
        /// First directory (absolute path matching a data= entry)
        source_a: PathBuf,
        /// Second directory (absolute path matching a data= entry)
        source_b: PathBuf,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Dump the VFS to a directory, run a command, then capture new/modified files to data-local.
    ///
    /// Use {} in the command arguments as a placeholder for the merged directory path.
    ///
    /// Example: vfstool run /tmp/merged -- tes3conv {} output.json
    Run {
        /// Directory to dump the merged VFS into
        merged_dir: PathBuf,

        /// Command and arguments to execute
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,

        /// Keep the merged directory after the command exits
        #[arg(long)]
        keep_merged: bool,

        /// Destination for captured files (defaults to data-local from openmw.cfg)
        #[arg(long)]
        output: Option<PathBuf>,

        /// Always copy files instead of hardlinking them.
        /// Hardlinks are used by default to avoid duplicating data on disk.
        #[arg(long)]
        copy: bool,

        /// Working directory for the child process.
        ///
        /// Defaults to the current working directory if not set.
        #[arg(long)]
        working_dir: Option<PathBuf>,
    },
}

/// Supported output formats
#[derive(Debug, ValueEnum, Clone)]
enum OutputFormat {
    Json,
    Yaml,
    Toml,
}

// --- Helpers ---

fn load_openmw_config(config_path: PathBuf) -> openmw_config::OpenMWConfiguration {
    match openmw_config::OpenMWConfiguration::new(Some(config_path)) {
        Err(e) => {
            eprintln!("Failed to load configuration file: {e}");
            std::process::exit(VFSToolExitCode::FailedToLoadOpenMWConfig.into());
        }
        Ok(cfg) => cfg,
    }
}

fn write_serialized<T: serde::Serialize>(
    path: Option<PathBuf>,
    format: OutputFormat,
    value: &T,
) -> io::Result<()> {
    let serialized = serialize_value(value, output_to_serialize_type(format))?;
    match path {
        None => println!("{serialized}"),
        Some(p) => {
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)?;
            }
            write!(fs::File::create(&p)?, "{serialized}")?;
        }
    }
    Ok(())
}

fn build_conflict_index(config_path: PathBuf) -> (VFS, ConflictIndex) {
    let cfg = load_openmw_config(config_path);
    let data_paths = cfg.data_directories();
    let archives: Vec<&str> = cfg
        .fallback_archives_iter()
        .map(|a| a.value().as_str())
        .collect();
    VFS::from_directories_with_conflict_index(data_paths, Some(archives))
}

fn validate_config_dir(dir: &PathBuf) -> io::Result<PathBuf> {
    let dir_metadata = std::fs::metadata(dir);

    match dir_metadata.is_ok() && dir_metadata.unwrap().is_dir() {
        false => {
            eprintln!(
                "[ ERROR ]: The requested openmw.cfg directory '{}' does not exist or is not a directory.",
                dir.display()
            );
        }
        true => {
            match fs::read_dir(dir)?
                .filter_map(|entry| entry.ok())
                .find(|entry| entry.file_name().eq_ignore_ascii_case("openmw.cfg"))
                .map(|entry| entry.path())
            {
                Some(cfg) => return Ok(cfg),
                None => {
                    eprintln!(
                        "[ ERROR ]: No openmw.cfg found in '{}'.",
                        dir.display()
                    );
                }
            }
        }
    }

    Err(std::io::Error::new(
        io::ErrorKind::NotFound,
        "Unable to resolve openmw.cfg path.",
    ))
}

fn output_to_serialize_type(format: OutputFormat) -> SerializeType {
    match format {
        OutputFormat::Json => SerializeType::Json,
        OutputFormat::Yaml => SerializeType::Yaml,
        OutputFormat::Toml => SerializeType::Toml,
    }
}

fn construct_vfs(config_path: PathBuf) -> VFS {
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

fn write_serialized_vfs(
    path: Option<PathBuf>,
    format: OutputFormat,
    files: &vfstool_lib::DisplayTree,
) -> io::Result<()> {
    let serialized = VFS::serialize_from_tree(files, output_to_serialize_type(format))?;
    match path {
        None => println!("{serialized}"),
        Some(path) => {
            let parent = path
                .parent()
                .expect("Failed to extract parent directory from output param!");
            fs::create_dir_all(parent)?;
            let mut file = fs::File::create(&path)?;
            write!(file, "{serialized}")?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let config_dir = args.config.unwrap_or(openmw_config::default_config_path());

    let resolved_config_dir = validate_config_dir(&config_dir)?;

    let vfs: VFS = construct_vfs(resolved_config_dir.clone());

    match args.command {
        Commands::Collapse {
            collapse_into,
            allow_copying,
            extract_archives,
            symbolic,
        } => {
            vfs.collapse_into(
                &collapse_into,
                &CollapseOptions { allow_copying, extract_archives, use_symlinks: symbolic },
            )?;
        }
        Commands::Extract {
            source_file,
            target_dir,
        } => {
            match vfs.extract_file(&source_file, &target_dir)? {
                None => eprintln!(
                    "{}Couldn't locate {} in the vfs!",
                    print::err_prefix(),
                    print::green(source_file.display()),
                ),
                Some(dest) => println!(
                    "{}Successfully extracted {} to {}",
                    print::success_prefix(),
                    print::green(source_file.display()),
                    print::blue(dest.parent().unwrap_or(&target_dir).display()),
                ),
            }
        }
        Commands::Find {
            path,
            format,
            output,
        } => {
            let path_string = normalize_path(&path).to_string_lossy().to_string();
            let tree = match vfs.find_by_regex(&path_string, args.use_relative) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(VFSToolExitCode::BadRegex.into());
                }
            };
            write_serialized_vfs(output, format, &tree)?;
        }
        Commands::FindFile {
            path,
            simple,
            only_physical,
        } => {
            let file = match vfs.get_file(&path) {
                Some(found_file) => found_file,
                None => {
                    if !simple {
                        eprintln!(
                            "{}Failed to locate {} in the provided VFS.",
                            print::err_prefix(),
                            print::blue(path.display()),
                        )
                    }
                    std::process::exit(VFSToolExitCode::FindFailed.into());
                }
            };

            let path_display = match file.is_archive() {
                true => {
                    if only_physical {
                        if !simple {
                            eprintln!(
                                "{}Failed to locate {} in loose files of the provided VFS.",
                                print::err_prefix(),
                                print::blue(path.display()),
                            )
                        }
                        std::process::exit(VFSToolExitCode::FileNotInLooseDirectories.into());
                    }
                    PathBuf::from(file.parent_archive_path().unwrap())
                        .join(&path)
                        .to_string_lossy()
                        .to_string()
                }
                false => file.path().to_string_lossy().to_string(),
            };

            if simple {
                print!("{}", path_display);
            } else {
                println!(
                    "{}Successfully found VFS File {} at path {}",
                    print::success_prefix(),
                    print::blue(&path.display()),
                    print::green(&path_display),
                )
            }
        }
        Commands::Remaining {
            filter_path,
            replacements_only,
            format,
            output,
        } => {
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

            let tree = vfs.remaining(&filter_path, replacements_only, &all_dirs, args.use_relative);
            write_serialized_vfs(output, format, &tree)?;
        }
        Commands::Conflicts { format, output } => {
            let (_, ci) = build_conflict_index(resolved_config_dir);
            let report = ci.conflicts_report(args.use_relative);
            write_serialized(output, format, &report)?;
        }
        Commands::Shadowed { format, output } => {
            let (_, ci) = build_conflict_index(resolved_config_dir);
            let report = ci.shadowed_report(args.use_relative);
            eprintln!("{} sources have fully shadowed files", report.sources.len());
            write_serialized(output, format, &report)?;
        }
        Commands::Which { path } => {
            let (vfs, ci) = build_conflict_index(resolved_config_dir);
            let normalized = normalize_path(&path).into_owned();

            let result = match ci.which(&vfs, &normalized) {
                Some(r) => r,
                None => {
                    eprintln!(
                        "{}VFS path '{}' not found.",
                        print::err_prefix(),
                        path.display()
                    );
                    std::process::exit(VFSToolExitCode::FindFailed.into());
                }
            };

            println!("VFS path: {}\n", normalized.display());
            if result.is_unique {
                println!("  {}  {} (no conflicts — only source)", print::green("WINNER"), result.winner);
            } else {
                println!("  {}  {}", print::green("WINNER"), result.winner);
                for src in &result.also_in {
                    println!("  also in {} (overridden)", src.display());
                }
            }
        }
        Commands::Stats => {
            let (vfs, ci) = build_conflict_index(resolved_config_dir);
            let report = ci.stats(&vfs);

            let path_width = report
                .rows
                .iter()
                .map(|r| r.source.display().to_string().len())
                .max()
                .unwrap_or(6)
                .max(6);

            println!(
                "{:<path_width$}  {:>6}  {:>9}  {:>10}",
                "Source", "Wins", "Overrides", "Overridden"
            );
            for row in &report.rows {
                println!(
                    "{:<path_width$}  {:>6}  {:>9}  {:>10}",
                    row.source.display(),
                    row.wins,
                    row.overrides,
                    row.overridden,
                );
            }
        }
        Commands::Diff { source_a, source_b, format, output } => {
            let (_, ci) = build_conflict_index(resolved_config_dir);
            let report = ci.diff_report(&source_a, &source_b);
            write_serialized(output, format, &report)?;
        }
        Commands::Run { merged_dir, command, keep_merged, output, copy, working_dir } => {
            let cfg = load_openmw_config(resolved_config_dir);

            let data_local: PathBuf = output.unwrap_or_else(|| {
                match cfg.data_local() {
                    Some(dir) => dir.parsed().to_path_buf(),
                    None => {
                        eprintln!(
                            "{}No data-local set in openmw.cfg; use --output to specify a destination.",
                            print::err_prefix()
                        );
                        std::process::exit(1);
                    }
                }
            });

            let merged = merged_dir;

            let (inner_result, subprocess_status) = (|| -> (Result<()>, Option<std::process::ExitStatus>) {
                eprintln!("Dumping VFS to {}...", merged.display());
                let (count, baseline) = match run_setup(&vfs, &merged, !copy) {
                    Ok(r) => r,
                    Err(e) => return (Err(e), None),
                };
                eprintln!("Dumped {count} files.");

                let substituted: Vec<String> = command
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
                if let Some(ref dir) = working_dir {
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

                let copied = match run_finalize(&merged, &baseline, &data_local) {
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

            if !keep_merged {
                let _ = fs::remove_dir_all(&merged);
            }

            inner_result?;
            std::process::exit(subprocess_status.and_then(|s| s.code()).unwrap_or(1));
        }
    }

    Ok(())
}
