// SPDX-License-Identifier: MIT OR Apache-2.0
mod cli;
mod commands;
mod config;
mod exit;
mod output;
mod print;

use clap::Parser;
use std::io::Result;

use cli::Cli;
use commands::run_command;
use config::resolve_config_path;

fn main() -> Result<()> {
    let args = Cli::parse();
    let resolved_config_dir = resolve_config_path(args.config)?;
    run_command(args.command, args.use_relative, resolved_config_dir)
}
