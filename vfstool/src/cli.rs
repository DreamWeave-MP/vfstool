// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "vfstool",
    about = "vfstool allows users to reconstruct and interact with OpenMW's virtual file system in any way they might see fit, using this application to locate files, serialize their VFS to most major text formats, extract files out of the vfs, and even collapse their VFS to a single directory for space savings."
)]
pub struct Cli {
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
    pub config: Option<PathBuf>,

    /// Whether or not to use relative paths in output
    #[arg(short = 'r', long)]
    pub use_relative: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Subcommands for `vfstool`
#[derive(Subcommand)]
pub enum Commands {
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
    /// Emit deterministic lock manifest for current winners.
    Lock {
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
        #[arg(long)]
        fail_on_drift: bool,
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

/// Supported output formats
#[derive(Debug, ValueEnum, Clone, Copy)]
pub enum OutputFormat {
    Json,
    Yaml,
    Toml,
}
