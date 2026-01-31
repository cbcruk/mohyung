mod commands;
mod core;
mod types;
mod utils;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mohyung",
    version,
    about = "Snapshot and restore node_modules as a single SQLite file"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pack node_modules into SQLite DB
    Pack {
        #[arg(short = 'o', long, default_value = "./node_modules.db")]
        output: String,

        #[arg(short = 's', long, default_value = "./node_modules")]
        source: String,

        #[arg(short = 'c', long, default_value = "6")]
        compression: u32,

        #[arg(long)]
        include_lockfile: bool,
    },

    /// Restore node_modules from SQLite DB
    Unpack {
        #[arg(short = 'i', long, default_value = "./node_modules.db")]
        input: String,

        #[arg(short = 'o', long, default_value = "./node_modules")]
        output: String,

        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Compare DB with current node_modules
    Status {
        #[arg(long, default_value = "./node_modules.db")]
        db: String,

        #[arg(short = 'n', long, default_value = "./node_modules")]
        node_modules: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Pack {
            output,
            source,
            compression,
            include_lockfile,
        } => commands::pack::pack(&types::PackOptions {
            output,
            source,
            compression_level: compression,
            include_lockfile,
        }),
        Commands::Unpack {
            input,
            output,
            force,
        } => commands::unpack::unpack(&types::UnpackOptions {
            input,
            output,
            force,
        }),
        Commands::Status { db, node_modules } => {
            commands::status::status(&db, &node_modules).map(|_| ())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
