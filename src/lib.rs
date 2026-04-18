use anyhow::Result;
use clap::Parser;

pub mod cli;
mod commands;
mod config;
mod devcontainer;
mod docker;
mod prompt;
mod system;

pub fn run() -> Result<()> {
    let cli = cli::Cli::parse();
    commands::dispatch(cli.command)
}
