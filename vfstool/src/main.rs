// SPDX-License-Identifier: GPL-3.0-only
//! Command-line application for inspecting and materializing an OpenMW-style virtual file system.
//!
//! `vfstool` reads `openmw.cfg`, builds the same loose-file/archive priority model used by `OpenMW`,
//! and exposes that model through focused commands. The binary rustdoc is intentionally useful: the
//! application should document its operational contracts in the same place as the library API. Fancy
//! idea, documenting the thing that users actually run.
//!
//! # Configuration
//!
//! `--config` points at the directory containing `openmw.cfg`:
//!
//! ```text
//! vfstool --config ~/.config/openmw explain textures/tx_bc_mudcrab.dds
//! ```
//!
//! For a nonstandard config filename, set `OPENMW_CONFIG` to the absolute config-file path instead:
//!
//! ```text
//! OPENMW_CONFIG=/tmp/profile/custom-openmw.cfg vfstool contributions
//! ```
//!
//! # Common workflows
//!
//! Explain the full provider chain for one normalized VFS key:
//!
//! ```text
//! vfstool explain textures/tx_bc_mudcrab.dds
//! ```
//!
//! Generate a deterministic winner lock and fail later when it drifts:
//!
//! ```text
//! vfstool lock -o vfs-lock.yaml
//! vfstool drift --fail-on-drift vfs-lock.yaml
//! ```
//!
//! Preview materialization without writing files:
//!
//! ```text
//! vfstool collapse --dry-run -f yaml /tmp/merged-preview
//! ```
//!
//! Run a tool against a merged tree. `{}` in child arguments is replaced with the merged directory:
//!
//! ```text
//! vfstool run /tmp/merged -- some-tool {} output.txt
//! ```
//!
//! By default `run` hardlinks loose files into the merged tree. Child tools that edit in place may
//! therefore mutate the original loose source files. Use `--copy` for tools that are not hardlink-safe.
//!
//! # Exit codes
//!
//! - `1`: `find-file` did not find the requested VFS path.
//! - `2`: `find-file --only_physical` found the path only inside an archive.
//! - `4`: `drift --fail-on-drift` detected drift.
//! - `6`: invalid regular expression.
//! - `7`: failed to load `openmw.cfg`.
//! - `8`: invalid input, such as an unknown source path.
//! - `9`: runtime failure while reading, writing, materializing, or starting/capturing a child command.
//!
//! `run` passes through the child process exit code after the child starts successfully.
mod cli;
mod commands;
mod config;
mod exit;
mod output;
mod print;

use clap::Parser;

use cli::Cli;
use commands::run_command;
use config::resolve_config_path;
use exit::VFSToolExitCode;

fn main() {
    let args = Cli::parse();
    let resolved_config_dir = match resolve_config_path(args.config) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Failed to resolve OpenMW config: {err}");
            std::process::exit(VFSToolExitCode::FailedToLoadOpenMWConfig.into());
        }
    };
    if let Err(err) = run_command(args.command, args.use_relative, resolved_config_dir) {
        eprintln!("vfstool: {err}");
        std::process::exit(VFSToolExitCode::RuntimeFailure.into());
    }
}
