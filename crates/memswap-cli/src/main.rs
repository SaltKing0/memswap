mod commands;

use clap::{Parser, Subcommand};

/// memswap — portable agent memory interchange format.
#[derive(Parser)]
#[command(name = "mem", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Structured JSON output on stdout.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create an empty memswap store.
    Init(commands::InitArgs),
    /// Export a harness's memory into a memswap store.
    Export(commands::ExportArgs),
    /// Import a memswap store into a harness's memory directory.
    Import(commands::ImportArgs),
    /// Verify store integrity / tamper-evidence.
    Verify(commands::VerifyArgs),
    /// Probe harnesses and report detect/read/write status.
    Doctor(commands::DoctorArgs),
    /// List built-in and loaded adapters.
    Adapters(commands::AdaptersArgs),
    /// Show version history of a store.
    Log(commands::LogArgs),
    /// Diff two stores or a store against HEAD.
    Diff(commands::DiffArgs),
    /// Rewrite a store to a new schema_version.
    Migrate(commands::MigrateArgs),
}

fn main() {
    let cli = Cli::parse();
    let code = commands::run(cli.command, cli.json);
    std::process::exit(code);
}
