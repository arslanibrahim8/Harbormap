//! The `harbormap` command line.

mod cli;
mod source;
mod status;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command, StatusArgs};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("harbormap: {error:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let input = source::load()?;
    match cli
        .command
        .unwrap_or(Command::Status(StatusArgs::default()))
    {
        Command::Status(args) => status::run(&input, &args, cli.json),
    }
}
