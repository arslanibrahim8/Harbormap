//! The `harbormap` command line.

mod card;
mod cli;
mod index;
mod learn;
mod source;
mod status;
mod why;

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
    let level = cli.level.into();
    match cli
        .command
        .unwrap_or(Command::Status(StatusArgs::default()))
    {
        Command::Status(args) => status::run(&source::load()?, &args, cli.json),
        Command::Why(args) => why::run(&source::load()?, &args, cli.json, level),
        // The code map reads the project, not the machine: no probe.
        Command::Index(args) => index::run(&args, cli.json),
        Command::Card(args) => card::run(&args, cli.json),
        Command::Learn(args) => learn::run(&args, cli.json),
    }
}
