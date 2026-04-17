// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    collections::HashMap,
    fs::{self, hard_link, metadata},
    io::{self, Result, Write},
    path::PathBuf,
};
use vfstool_lib::{
    changed_files, snapshot_directory, ConflictIndex, SerializeType, normalize_path,
    serialize_value, vfs::VFS,
};

#[cfg(unix)]
use std::os::unix::fs::symlink as soft_link;

#[cfg(windows)]
use std::os::windows::fs::symlink_file as soft_link;

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
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const BLUE: &str = "\x1b[34m";
    pub const RESET: &str = "\x1b[0m";

    pub const fn err_prefix() -> &'static str {
        concat!("\x1b[31m", "[ ERROR ]", "\x1b[0m", ": ")
    }

    pub const fn success_prefix() -> &'static str {
        concat!("\x1b[32m", "[ SUCCESS ]", "\x1b[0m", ": ")
    }

    pub fn red<S: std::fmt::Display>(input: S) -> String {
        format!("{RED}{input}{RESET}")
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
    },
}

/// Supported output formats
#[derive(Debug, ValueEnum, Clone)]
enum OutputFormat {
    Json,
    Yaml,
    Toml,
}

/// Type of search to do when finding a file
#[derive(Debug, PartialEq, ValueEnum, Clone)]
enum FindType {
    Contains,
    Extension,
    Folder,
    Prefix,
    Stem,
    StemExact,
    Name,
    NameExact,
}

// --- Serialization structs for new commands ---

#[derive(serde::Serialize)]
struct ConflictsReport {
    sources: Vec<ConflictSourceEntry>,
}

#[derive(serde::Serialize)]
struct ConflictSourceEntry {
    path: PathBuf,
    overrides: Vec<PathBuf>,
    overridden_by: Vec<PathBuf>,
}

#[derive(serde::Serialize)]
struct ShadowedReport {
    sources: Vec<ShadowedSource>,
}

#[derive(serde::Serialize)]
struct ShadowedSource {
    path: PathBuf,
    shadowed_files: Vec<PathBuf>,
}

#[derive(serde::Serialize)]
struct DiffReport {
    source_a: PathBuf,
    source_b: PathBuf,
    higher_priority: PathBuf,
    shared: Vec<PathBuf>,
    only_in_a: Vec<PathBuf>,
    only_in_b: Vec<PathBuf>,
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
    let dir_metadata = metadata(dir);

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

fn filter_data_paths(to_keep: &PathBuf, paths: &mut Vec<PathBuf>) {
    let normalized_input = normalize_path(&to_keep);
    paths.retain(|path| normalize_path(&path).eq(&normalized_input))
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
            if metadata(&collapse_into).is_err() {
                fs::create_dir_all(&collapse_into)?;
            };

            vfs.iter().for_each(|(relative_path, file)| {
                let merged_path = collapse_into.join(relative_path);
                let merged_dir = merged_path.parent().unwrap();

                if metadata(&merged_dir).is_err() {
                    fs::create_dir_all(&merged_dir).unwrap();
                };

                if file.is_loose() {
                    if !file.path().exists() {
                        eprintln!(
                            "Skipping {}: source file no longer exists at {}",
                            relative_path.display(),
                            file.path().display()
                        );
                        return;
                    }

                    if let Err(e) = fs::remove_file(&merged_path) {
                        if e.kind() != io::ErrorKind::NotFound {
                            eprintln!(
                                "Failed to remove existing file at {}: {}",
                                merged_path.display(), e
                            );
                            return;
                        }
                    }

                    // Since we extract files *out of* BSA archives
                    // Don't bother including them in the collapsed directory
                    if let Some(extension) = file.path().extension() {
                        let extension = extension.to_ascii_lowercase();
                        let file_name = file.file_name().unwrap_or_default().to_ascii_lowercase();

                        if (extension == "bsa" || extension == "ba2") && extract_archives && file_name != "archiveinvalidationinvalidated!.bsa" {
                            println!("Skipping archive {}", file.file_name().unwrap().to_string_lossy());
                            return;
                        }
                    }

                    let link_fn = if symbolic {
                        soft_link
                    } else {
                        hard_link
                    };

                    if let Err(error) = link_fn(file.path(), &merged_path) {
                        eprintln!(
                            "Symlink attempt for {} failed due to error: {}",
                            file.path().display(),
                            error.to_string()
                        );

                        if allow_copying {
                            if let Err(error) = fs::copy(file.path(), &merged_path) {
                                eprintln!(
                                    "Fallback file copying was enabled, but copying {} to {} failed due to {}!",
                                    file.path().display(),
                                    merged_path.display(),
                                    error.to_string()
                                );
                            }
                        }
                    } else {
                        println!("Successfully wrote {} to {}", file.path().display(), merged_path.display());
                    };
                } else {
                    if !extract_archives {
                        println!(
                            "Skipping {}, which is loaded from a BSA file at: {}",
                            relative_path.display(),
                            file.parent_archive_path().unwrap()
                        )
                    } else {
                        match file.open() {
                            Ok(mut data) => {
                                let mut buf: Vec<u8> = Vec::new();
                                match data.read_to_end(&mut buf) {
                                    Err(error) => eprintln!(
                                        "Failed to read archived file {}: {}",
                                        relative_path.display(),
                                        error
                                    ),
                                    Ok(_) => {
                                        if let Err(error) = fs::write(&merged_path, buf) {
                                            eprintln!(
                                                "Extracting archived file {} to {} failed: {}",
                                                relative_path.display(),
                                                merged_path.display(),
                                                error
                                            );
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                eprintln!("Failed to open archived file {}: {}", relative_path.display(), error)
                            }
                        };
                    }
                }
            });
        }
        Commands::Extract {
            source_file,
            target_dir,
        } => match vfs.get_file(&source_file) {
            Some(file) => {
                let mut dir_meta = metadata(&target_dir);

                if dir_meta.is_err() {
                    fs::create_dir_all(&target_dir)?;
                    dir_meta = metadata(&target_dir);
                }

                let dir_meta = dir_meta?;

                if dir_meta.is_dir() {
                    match source_file.file_name() {
                        Some(name) => {
                            let target_path = target_dir.join(name);

                            if file.is_loose() {
                                if let Err(error) = fs::copy(file.path(), &target_path) {
                                    eprintln!(
                                        "{}Failed extracting loose file from the vfs: {}",
                                        print::err_prefix(),
                                        print::red(error.to_string()),
                                    );
                                } else {
                                    println!(
                                        "{}Successfully extracted {} to {}",
                                        print::success_prefix(),
                                        print::green(file.path().display()),
                                        print::blue(target_dir.display())
                                    );
                                };
                            } else {
                                match file.open() {
                                    Ok(mut data) => {
                                        let mut buf: Vec<u8> = Vec::new();
                                        match data.read_to_end(&mut buf) {
                                            Err(error) => eprintln!(
                                                "{}Failed to read archived file {}: {}",
                                                print::err_prefix(),
                                                print::green(source_file.display()),
                                                print::red(error.to_string()),
                                            ),
                                            Ok(_) => {
                                                if let Err(error) = fs::write(&target_path, buf) {
                                                    eprintln!(
                                                        "{}Extracting archived file {} to {} failed: {}",
                                                        print::err_prefix(),
                                                        print::green(source_file.display()),
                                                        print::blue(target_path.display()),
                                                        print::red(error.to_string()),
                                                    );
                                                } else {
                                                    println!(
                                                        "{}Successfully extracted {} to {}",
                                                        print::success_prefix(),
                                                        print::green(file.path().display()),
                                                        print::blue(target_dir.display()),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "{}Failed to open archived file {}: {}",
                                            print::err_prefix(),
                                            print::green(source_file.display()),
                                            print::red(error.to_string()),
                                        )
                                    }
                                }
                            }
                        }
                        None => eprintln!(
                            "{}Source file {} does not have a file name! Cannot extract it!",
                            print::err_prefix(),
                            print::green(source_file.display()),
                        ),
                    };
                } else {
                    eprintln!(
                        "{}Provided argument {} is not a directory! Cannot extract here!",
                        print::err_prefix(),
                        print::green(target_dir.display()),
                    );
                }
            }
            None => eprintln!(
                "{}Couldn't locate {} in the vfs!",
                print::err_prefix(),
                print::green(source_file.display()),
            ),
        },
        Commands::Find {
            path,
            format,
            output,
        } => {
            // Lossy compare could produce false positives, but only if there are non-unicode
            // characters at the same position in both the path and string being matched and the
            // rest of the string is the same
            let path_string = normalize_path(&path).to_string_lossy().to_string();
            let path_regex: regex::Regex = match regex::RegexBuilder::new(&path_string)
                .case_insensitive(true)
                .build()
            {
                Ok(regex) => regex,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(VFSToolExitCode::BadRegex.into());
                }
            };

            let tree = vfs.tree_filtered(args.use_relative, |_key, file| {
                let normalized = normalize_path(file.path());
                path_regex.is_match(&normalized.to_string_lossy())
            });

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

                    // Ugly, make a dedicated enum for exit values later
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

            let mut paths = config
                .data_directories_iter()
                .map(|dir| dir.parsed().to_owned())
                .collect();

            filter_data_paths(&filter_path, &mut paths);

            let filtered_vfs = VFS::from_directories(&paths, None);
            let filter_normalized = normalize_path(&filter_path).into_owned();

            let files_remaining = vfs.tree_filtered(args.use_relative, |key, file| {
                if replacements_only {
                    // A replacement: filter_path has a file at this key, but the full VFS
                    // serves it from somewhere else (i.e., a later directory won).
                    filtered_vfs.contains(key)
                        && !normalize_path(file.path()).starts_with(&filter_normalized)
                } else {
                    // Still loaded from filter_path — not overridden by anything later.
                    normalize_path(file.path()).starts_with(&filter_normalized)
                }
            });

            write_serialized_vfs(output, format, &files_remaining)?;
        }
        Commands::Conflicts { format, output } => {
            let (_, ci) = build_conflict_index(resolved_config_dir);

            let report = ConflictsReport {
                sources: ci.sources.iter().enumerate().map(|(i, src)| {
                    let resolve = |p: &PathBuf| -> PathBuf {
                        if args.use_relative { p.clone() } else { src.join(p) }
                    };
                    let mut overrides: Vec<PathBuf> =
                        ci.conflicts[i].overrides.iter().map(resolve).collect();
                    let mut overridden_by: Vec<PathBuf> =
                        ci.conflicts[i].overridden_by.iter().map(resolve).collect();
                    overrides.sort();
                    overridden_by.sort();
                    ConflictSourceEntry { path: src.clone(), overrides, overridden_by }
                }).collect(),
            };

            write_serialized(output, format, &report)?;
        }
        Commands::Shadowed { format, output } => {
            let (_, ci) = build_conflict_index(resolved_config_dir);

            let sources: Vec<ShadowedSource> = ci
                .sources
                .iter()
                .enumerate()
                .filter_map(|(i, src)| {
                    if !ci.conflicts[i].is_overridden() {
                        return None;
                    }
                    let mut shadowed_files: Vec<PathBuf> =
                        ci.conflicts[i].overridden_by.iter().cloned().collect();
                    shadowed_files.sort();
                    Some(ShadowedSource { path: src.clone(), shadowed_files })
                })
                .collect();

            let total = sources.len();
            eprintln!("{total} sources have fully shadowed files");

            write_serialized(output, format, &ShadowedReport { sources })?;
        }
        Commands::Which { path } => {
            let (vfs, ci) = build_conflict_index(resolved_config_dir);
            let normalized = normalize_path(&path).into_owned();

            let winner = match vfs.get_file(&normalized) {
                Some(f) => f,
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

            let winner_display = if winner.is_loose() {
                winner.path().display().to_string()
            } else {
                winner.parent_archive_path().unwrap_or_default()
            };

            let source_indices = ci.sources_containing(&normalized);

            if source_indices.is_empty() {
                println!("  {}  {} (no conflicts — only source)", print::green("WINNER"), winner_display);
            } else {
                println!("  {}  {}", print::green("WINNER"), winner_display);

                // Find which source the winner belongs to so we can skip it in "also in"
                let winner_src_idx = if winner.is_loose() {
                    ci.sources.iter().position(|src| winner.path().starts_with(src))
                } else {
                    winner.parent_archive_path().and_then(|ap| {
                        ci.sources.iter().position(|src| src == &PathBuf::from(&ap))
                    })
                };

                for &idx in source_indices {
                    if Some(idx) == winner_src_idx {
                        continue;
                    }
                    println!("  also in {} (overridden)", ci.sources[idx].display());
                }
            }
        }
        Commands::Stats => {
            let (vfs, ci) = build_conflict_index(resolved_config_dir);

            let mut wins: HashMap<usize, usize> = HashMap::new();
            for (_, file) in vfs.iter() {
                let source_idx = if file.is_loose() {
                    ci.sources.iter().position(|src| file.path().starts_with(src))
                } else {
                    file.parent_archive_path().and_then(|ap| {
                        ci.sources
                            .iter()
                            .position(|src| src.to_string_lossy() == ap.as_str())
                    })
                };
                if let Some(idx) = source_idx {
                    *wins.entry(idx).or_insert(0) += 1;
                }
            }

            // Compute column widths
            let path_width = ci
                .sources
                .iter()
                .map(|s| s.display().to_string().len())
                .max()
                .unwrap_or(6)
                .max(6);

            println!(
                "{:<path_width$}  {:>6}  {:>9}  {:>10}",
                "Source", "Wins", "Overrides", "Overridden"
            );
            for (i, src) in ci.sources.iter().enumerate() {
                let w = wins.get(&i).copied().unwrap_or(0);
                let overrides = ci.conflicts[i].overrides.len();
                let overridden = ci.conflicts[i].overridden_by.len();
                println!(
                    "{:<path_width$}  {:>6}  {:>9}  {:>10}",
                    src.display(),
                    w,
                    overrides,
                    overridden,
                );
            }
        }
        Commands::Diff { source_a, source_b, format, output } => {
            let vfs_a = VFS::from_directories([&source_a], None);
            let vfs_b = VFS::from_directories([&source_b], None);

            let keys_a: std::collections::HashSet<PathBuf> =
                vfs_a.iter().map(|(k, _)| k.clone()).collect();
            let keys_b: std::collections::HashSet<PathBuf> =
                vfs_b.iter().map(|(k, _)| k.clone()).collect();

            let mut shared: Vec<PathBuf> = keys_a.intersection(&keys_b).cloned().collect();
            let mut only_in_a: Vec<PathBuf> = keys_a.difference(&keys_b).cloned().collect();
            let mut only_in_b: Vec<PathBuf> = keys_b.difference(&keys_a).cloned().collect();
            shared.sort();
            only_in_a.sort();
            only_in_b.sort();

            // Determine load-order priority from the full ConflictIndex
            let (_, ci) = build_conflict_index(resolved_config_dir);
            let idx_a = ci.sources.iter().position(|s| s == &source_a);
            let idx_b = ci.sources.iter().position(|s| s == &source_b);
            let higher_priority = match (idx_a, idx_b) {
                (Some(a), Some(b)) => {
                    if a > b { source_a.clone() } else { source_b.clone() }
                }
                // If either path isn't in the index just fall back to source_b
                _ => source_b.clone(),
            };

            let report = DiffReport { source_a, source_b, higher_priority, shared, only_in_a, only_in_b };
            write_serialized(output, format, &report)?;
        }
        Commands::Run { merged_dir, command, keep_merged, output, copy } => {
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

            fs::create_dir_all(&merged_dir)?;
            let merged = merged_dir;

            // Run all fallible logic in a closure so cleanup always executes,
            // even on early error. std::process::exit bypasses destructors, so
            // we call remove_dir_all explicitly rather than relying on Drop.
            let (inner_result, subprocess_status) = (|| -> (Result<()>, Option<std::process::ExitStatus>) {
                eprintln!("Dumping VFS to {}...", merged.display());
                let count = match vfs.dump_to_directory(&merged, !copy) {
                    Ok(c) => c,
                    Err(e) => return (Err(e), None),
                };
                eprintln!("Dumped {count} files.");

                let baseline = match snapshot_directory(&merged) {
                    Ok(b) => b,
                    Err(e) => return (Err(e), None),
                };

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

                let status = match std::process::Command::new(&substituted[0])
                    .args(&substituted[1..])
                    .status()
                {
                    Ok(s) => s,
                    Err(e) => return (Err(e), None),
                };

                if !status.success() {
                    eprintln!(
                        "vfstool: subprocess exited with {status}, not capturing files."
                    );
                    return (Ok(()), Some(status));
                }

                let changed = match changed_files(&merged, &baseline) {
                    Ok(c) => c,
                    Err(e) => return (Err(e), Some(status)),
                };

                if changed.is_empty() {
                    eprintln!("No files changed.");
                } else {
                    eprintln!(
                        "Capturing {} changed file(s) to {}...",
                        changed.len(),
                        data_local.display()
                    );
                    for rel in &changed {
                        let dest = data_local.join(rel);
                        if let Some(parent) = dest.parent() {
                            if let Err(e) = fs::create_dir_all(parent) {
                                return (Err(e), Some(status));
                            }
                        }
                        if let Err(e) = fs::copy(merged.join(rel), &dest) {
                            return (Err(e), Some(status));
                        }
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
