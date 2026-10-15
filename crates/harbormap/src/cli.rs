//! Command-line arguments.

use clap::{Args, Parser, Subcommand, ValueEnum};

use hm_core::signature::Level;

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

    /// How much detail explanations give.
    #[arg(long, global = true, value_enum, default_value_t = LevelArg::Builder)]
    pub level: LevelArg,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show what runs for this project and elsewhere on this computer.
    Status(StatusArgs),
    /// Explain who holds a port or what a service is.
    Why(WhyArgs),
}

#[derive(Debug, Default, Args)]
pub struct StatusArgs {
    /// List every project, system program and unknown owner instead of a summary.
    #[arg(long)]
    pub machine: bool,
}

#[derive(Debug, Args)]
pub struct WhyArgs {
    /// A port number, a service name, or project/service.
    pub target: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LevelArg {
    Beginner,
    Builder,
    Expert,
}

impl From<LevelArg> for Level {
    fn from(level: LevelArg) -> Self {
        match level {
            LevelArg::Beginner => Level::Beginner,
            LevelArg::Builder => Level::Builder,
            LevelArg::Expert => Level::Expert,
        }
    }
}
