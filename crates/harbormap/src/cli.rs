//! Command-line arguments.

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "harbormap",
    version,
    about = "Know what's running, why it's broken, and keep your AI agents from breaking it."
)]
pub struct Cli {
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show what runs for this project and elsewhere on this computer.
    Status(StatusArgs),
}

#[derive(Debug, Default, Args)]
pub struct StatusArgs {
    /// List every project, system program and unknown owner instead of a summary.
    #[arg(long)]
    pub machine: bool,
}
