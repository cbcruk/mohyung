use clap::{Parser, Subcommand};
use mohyung::{commands, types};

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

        #[arg(short = 'c', long, default_value = "6", value_parser = clap::value_parser!(u32).range(1..=9))]
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

        /// Restore by linking from a local content-addressable store (hardlink,
        /// copy fallback) instead of writing each file. Fast for repeated restores.
        #[arg(short = 'l', long)]
        link: bool,

        /// Prefer copy-on-write reflinks over hardlinks where the filesystem
        /// supports them (Btrfs/XFS/APFS). Safer for editing restored files;
        /// implies --link.
        #[arg(long)]
        reflink: bool,
    },

    /// Compare DB with current node_modules
    Status {
        #[arg(long, default_value = "./node_modules.db")]
        db: String,

        #[arg(short = 'n', long, default_value = "./node_modules")]
        node_modules: String,

        #[arg(short = 'v', long)]
        verbose: bool,
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
            link,
            reflink,
        } => commands::unpack::unpack(&types::UnpackOptions {
            input,
            output,
            force,
            link,
            reflink,
        }),
        Commands::Status {
            db,
            node_modules,
            verbose,
        } => commands::status::status(&db, &node_modules, verbose).map(|_| ()),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
