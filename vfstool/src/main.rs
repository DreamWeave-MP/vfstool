// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fs,
    io::{self, Result, Write},
    path::{Path, PathBuf},
};
use vfstool_lib::{
    CandidatePlanOpts, CollapseOptions, ConflictIndex, LayerIndex, Policy, ReorderOp, Rule,
    SerializeType, SimOpts, SourceKind, VfsLock, normalize_path, run_finalize, run_setup,
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
        /// Full relative path to a VFS file, eg `meshes/xbase_anim.nif`
        source_file: PathBuf,

        /// Directory to extract the file to
        target_dir: PathBuf,
    },
    /// Given some VFS path, like `meshes/xbase_anim.nif`, return its absolute path (if found)
    FindFile {
        /// Full (relative) VFS Path to query.
        /// Returns the absolute path, of the file referenced by this VFS path. EG:
        ///
        /// vfstool find-file `meshes/xbase_anim.nif`
        ///
        /// C:\Games\Morrowind\Data `Files\Meshes\XBase_Anim.nif`
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
        /// Relative VFS path to query, e.g. `textures/tx_bc_mudcrab.dds`
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
    /// Show full provider chain for one VFS path.
    Provenance {
        /// Relative VFS path to query.
        path: PathBuf,
        /// Include content hashes where available.
        #[arg(long)]
        hashes: bool,
        /// Output format when serializing as text.
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        /// Optional path to save the output.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Report semantic (content-aware) conflicts.
    SemanticConflicts {
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Emit deterministic lock manifest for current winners.
    Lock {
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Evaluate a YAML policy document against the current VFS.
    Verify {
        /// Path to YAML policy file.
        policy: PathBuf,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Simulate swapping two sources in load order.
    SimulateSwap {
        /// Source path A.
        source_a: PathBuf,
        /// Source path B.
        source_b: PathBuf,
        /// Optional impact bucket globs (repeat this flag).
        #[arg(long = "bucket")]
        buckets: Vec<String>,
        /// Maximum number of changed keys included in output sample.
        #[arg(long, default_value_t = 100)]
        sample_limit: usize,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Simulate moving one source before another in load order.
    SimulateMoveBefore {
        /// Source path to move.
        source: PathBuf,
        /// Destination source before which `source` is inserted.
        before: PathBuf,
        /// Optional impact bucket globs (repeat this flag).
        #[arg(long = "bucket")]
        buckets: Vec<String>,
        /// Maximum number of changed keys included in output sample.
        #[arg(long, default_value_t = 100)]
        sample_limit: usize,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Simulate moving one source after another in load order.
    SimulateMoveAfter {
        /// Source path to move.
        source: PathBuf,
        /// Destination source after which `source` is inserted.
        after: PathBuf,
        /// Optional impact bucket globs (repeat this flag).
        #[arg(long = "bucket")]
        buckets: Vec<String>,
        /// Maximum number of changed keys included in output sample.
        #[arg(long, default_value_t = 100)]
        sample_limit: usize,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Simulate a full explicit source order from a file.
    SimulateFullOrder {
        /// Text file with one absolute source path per line.
        ///
        /// Empty lines and lines beginning with '#' are ignored.
        #[arg(long)]
        order_file: PathBuf,
        /// Optional impact bucket globs (repeat this flag).
        #[arg(long = "bucket")]
        buckets: Vec<String>,
        /// Maximum number of changed keys included in output sample.
        #[arg(long, default_value_t = 100)]
        sample_limit: usize,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compare current VFS state to a lock manifest.
    Drift {
        /// Path to lock manifest (yaml/json/toml; inferred from extension).
        lock_file: PathBuf,
        /// If set, drift causes exit code 4.
        #[arg(long, default_value_t = true)]
        fail_on_drift: bool,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Preflight one candidate directory before adding it to the load order.
    PlanCandidate {
        /// Candidate data directory to evaluate.
        candidate_dir: PathBuf,
        /// Disable semantic (content hash) comparison for conflicts.
        #[arg(long)]
        no_semantic: bool,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(serde::Deserialize)]
struct PolicyDoc {
    rules: Vec<PolicyRuleDoc>,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PolicyRuleDoc {
    WinnerMustMatch {
        path_glob: String,
        source_glob: String,
    },
    WinnerMustNotMatch {
        path_glob: String,
        source_glob: String,
    },
    MustExist {
        path_glob: String,
    },
    MustBeUnique {
        path_glob: String,
    },
    WinnerKindMustBe {
        path_glob: String,
        kind: String,
    },
    MaxOverrideDepth {
        path_glob: String,
        max: usize,
    },
}

/// Supported output formats
#[derive(Debug, ValueEnum, Clone, Copy)]
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

fn build_layer_index(config_path: PathBuf) -> (VFS, LayerIndex) {
    let cfg = load_openmw_config(config_path);
    let data_paths = cfg.data_directories();
    let archives: Vec<&str> = cfg
        .fallback_archives_iter()
        .map(|a| a.value().as_str())
        .collect();
    VFS::from_directories_with_layer_index(data_paths, Some(archives))
}

fn parse_policy(path: &PathBuf) -> io::Result<Policy> {
    let text = std::fs::read_to_string(path)?;
    let doc: PolicyDoc = serde_yaml::from_str(&text).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid policy yaml: {e}"),
        )
    })?;

    let mut rules = Vec::with_capacity(doc.rules.len());
    for rule in doc.rules {
        let mapped = match rule {
            PolicyRuleDoc::WinnerMustMatch {
                path_glob,
                source_glob,
            } => Rule::WinnerMustMatch {
                path_glob,
                source_glob,
            },
            PolicyRuleDoc::WinnerMustNotMatch {
                path_glob,
                source_glob,
            } => Rule::WinnerMustNotMatch {
                path_glob,
                source_glob,
            },
            PolicyRuleDoc::MustExist { path_glob } => Rule::MustExist { path_glob },
            PolicyRuleDoc::MustBeUnique { path_glob } => Rule::MustBeUnique { path_glob },
            PolicyRuleDoc::WinnerKindMustBe { path_glob, kind } => {
                let kind = match kind.as_str() {
                    "loose_dir" => SourceKind::LooseDir,
                    "archive" => SourceKind::Archive,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "invalid winner kind '{kind}', expected 'loose_dir' or 'archive'"
                            ),
                        ));
                    }
                };
                Rule::WinnerKindMustBe { path_glob, kind }
            }
            PolicyRuleDoc::MaxOverrideDepth { path_glob, max } => {
                Rule::MaxOverrideDepth { path_glob, max }
            }
        };
        rules.push(mapped);
    }

    Ok(Policy { rules })
}

fn parse_order_file(path: &Path) -> io::Result<Vec<PathBuf>> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(PathBuf::from)
        .collect())
}

fn parse_lock_file(path: &Path) -> io::Result<VfsLock> {
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

fn run_simulation_command(
    resolved_config_dir: PathBuf,
    op: ReorderOp,
    buckets: Vec<String>,
    sample_limit: usize,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> io::Result<()> {
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let opts = SimOpts {
        sample_limit,
        impact_buckets: buckets,
    };

    match layer.simulate_with_opts(&vfs, op, &opts) {
        Ok(delta) => write_serialized(output, format, &delta),
        Err(err) if err.kind() == io::ErrorKind::InvalidInput => {
            eprintln!("{}{}", print::err_prefix(), err);
            std::process::exit(2);
        }
        Err(err) => Err(err),
    }
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

fn handle_collapse(
    vfs: &VFS,
    collapse_into: &Path,
    allow_copying: bool,
    extract_archives: bool,
    symbolic: bool,
) -> Result<()> {
    vfs.collapse_into(
        collapse_into,
        &CollapseOptions {
            allow_copying,
            extract_archives,
            use_symlinks: symbolic,
        },
    )
}

fn handle_extract(vfs: &VFS, source_file: &Path, target_dir: &Path) -> Result<()> {
    match vfs.extract_file(source_file, target_dir)? {
        None => eprintln!(
            "{}Couldn't locate {} in the vfs!",
            print::err_prefix(),
            print::green(source_file.display()),
        ),
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
        print!("{path_display}");
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

fn handle_which(resolved_config_dir: PathBuf, path: &PathBuf) {
    let (vfs, ci) = build_conflict_index(resolved_config_dir);
    let normalized = normalize_path(&path).into_owned();
    let Some(result) = ci.which(&vfs, &normalized) else {
        eprintln!(
            "{}VFS path '{}' not found.",
            print::err_prefix(),
            path.display()
        );
        std::process::exit(VFSToolExitCode::FindFailed.into());
    };

    println!("VFS path: {}\n", normalized.display());
    if result.is_unique {
        println!(
            "  {}  {} (no conflicts — only source)",
            print::green("WINNER"),
            result.winner
        );
    } else {
        println!("  {}  {}", print::green("WINNER"), result.winner);
        for src in &result.also_in {
            println!("  also in {} (overridden)", src.display());
        }
    }
}

fn handle_stats(resolved_config_dir: PathBuf) {
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

struct RunParams<'a> {
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
            std::process::exit(1);
        }
    });

    let merged = params.merged_dir;
    let (inner_result, subprocess_status) =
        (|| -> (Result<()>, Option<std::process::ExitStatus>) {
            eprintln!("Dumping VFS to {}...", merged.display());
            let (count, baseline) = match run_setup(vfs, &merged, !params.copy) {
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

    if !params.keep_merged {
        let _ = fs::remove_dir_all(&merged);
    }
    inner_result?;
    std::process::exit(subprocess_status.and_then(|s| s.code()).unwrap_or(1));
}

fn handle_provenance(
    resolved_config_dir: PathBuf,
    path: &PathBuf,
    hashes: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let normalized = normalize_path(&path).into_owned();
    let Some(chain) = layer.provenance(&vfs, &normalized, hashes)? else {
        eprintln!(
            "{}VFS path '{}' not found.",
            print::err_prefix(),
            path.display()
        );
        std::process::exit(VFSToolExitCode::FindFailed.into());
    };
    write_serialized(output, format, &chain)
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
    eprintln!("{} sources have fully shadowed files", report.sources.len());
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

fn handle_semantic(
    resolved_config_dir: PathBuf,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let report = layer.semantic_conflicts(&vfs)?;
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

fn handle_verify(
    resolved_config_dir: PathBuf,
    policy: &PathBuf,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let policy = parse_policy(policy)?;
    let result = policy.evaluate(&layer, &vfs)?;
    let has_violations = !result.violations.is_empty();
    write_serialized(output, format, &result)?;
    if has_violations {
        std::process::exit(3);
    }
    Ok(())
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
        std::process::exit(4);
    }
    Ok(())
}

fn handle_plan_candidate(
    resolved_config_dir: PathBuf,
    candidate_dir: &Path,
    no_semantic: bool,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<()> {
    let (vfs, layer) = build_layer_index(resolved_config_dir);
    let plan = layer.plan_candidate_directory(
        &vfs,
        candidate_dir,
        CandidatePlanOpts {
            include_semantic: !no_semantic,
        },
    )?;
    write_serialized(output, format, &plan)
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
        } => {
            handle_collapse(
                vfs,
                collapse_into.as_path(),
                allow_copying,
                extract_archives,
                symbolic,
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
        other => Ok(Some(other)),
    }
}

fn run_analysis_command(
    command: Commands,
    use_relative: bool,
    resolved_config_dir: PathBuf,
) -> Result<()> {
    match command {
        Commands::Conflicts { format, output } => {
            handle_conflicts(resolved_config_dir, use_relative, format, output)
        }
        Commands::Shadowed { format, output } => {
            handle_shadowed(resolved_config_dir, use_relative, format, output)
        }
        Commands::Which { path } => {
            handle_which(resolved_config_dir, &path);
            Ok(())
        }
        Commands::Stats => {
            handle_stats(resolved_config_dir);
            Ok(())
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
        ),
        Commands::Provenance {
            path,
            hashes,
            format,
            output,
        } => handle_provenance(resolved_config_dir, &path, hashes, format, output),
        Commands::SemanticConflicts { format, output } => {
            handle_semantic(resolved_config_dir, format, output)
        }
        Commands::Lock { format, output } => handle_lock(resolved_config_dir, format, output),
        Commands::Verify {
            policy,
            format,
            output,
        } => handle_verify(resolved_config_dir, &policy, format, output),
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
        ),
        Commands::PlanCandidate {
            candidate_dir,
            no_semantic,
            format,
            output,
        } => handle_plan_candidate(
            resolved_config_dir,
            candidate_dir.as_path(),
            no_semantic,
            format,
            output,
        ),
        Commands::Collapse { .. }
        | Commands::Extract { .. }
        | Commands::Find { .. }
        | Commands::FindFile { .. }
        | Commands::Remaining { .. }
        | Commands::Run { .. }
        | Commands::SimulateSwap { .. }
        | Commands::SimulateMoveBefore { .. }
        | Commands::SimulateMoveAfter { .. }
        | Commands::SimulateFullOrder { .. } => Ok(()),
    }
}

fn run_command(
    command: Commands,
    use_relative: bool,
    resolved_config_dir: PathBuf,
    vfs: &VFS,
) -> Result<()> {
    let Some(command) =
        run_core_vfs_command(command, vfs, use_relative, resolved_config_dir.clone())?
    else {
        return Ok(());
    };

    match command {
        Commands::SimulateSwap {
            source_a,
            source_b,
            buckets,
            sample_limit,
            format,
            output,
        } => run_simulation_command(
            resolved_config_dir,
            ReorderOp::Swap(source_a, source_b),
            buckets,
            sample_limit,
            format,
            output,
        ),
        Commands::SimulateMoveBefore {
            source,
            before,
            buckets,
            sample_limit,
            format,
            output,
        } => run_simulation_command(
            resolved_config_dir,
            ReorderOp::MoveBefore { source, before },
            buckets,
            sample_limit,
            format,
            output,
        ),
        Commands::SimulateMoveAfter {
            source,
            after,
            buckets,
            sample_limit,
            format,
            output,
        } => run_simulation_command(
            resolved_config_dir,
            ReorderOp::MoveAfter { source, after },
            buckets,
            sample_limit,
            format,
            output,
        ),
        Commands::SimulateFullOrder {
            order_file,
            buckets,
            sample_limit,
            format,
            output,
        } => {
            let order = parse_order_file(&order_file)?;
            run_simulation_command(
                resolved_config_dir,
                ReorderOp::FullOrder(order),
                buckets,
                sample_limit,
                format,
                output,
            )
        }
        _ => run_analysis_command(command, use_relative, resolved_config_dir),
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let config_dir = args.config.unwrap_or(openmw_config::default_config_path());
    let resolved_config_dir = validate_config_dir(&config_dir)?;
    let vfs = construct_vfs(resolved_config_dir.clone());
    run_command(args.command, args.use_relative, resolved_config_dir, &vfs)
}
