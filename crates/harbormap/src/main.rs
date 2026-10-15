//! The `harbormap` command line.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "harbormap",
    version,
    about = "Know what's running, why it's broken, and keep your AI agents from breaking it."
)]
struct Cli {}

fn main() {
    Cli::parse();
}
