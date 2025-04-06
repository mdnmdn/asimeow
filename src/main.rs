use anyhow::Result;
use asimeow::config;
use asimeow::explorer;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    author = "mdnmdn",
    about = "A tool for managing macOS Time Machine exclusions for developer projects",
    long_about = None
)]
struct Args {
    /// Path to the config file
    #[arg(short, long, default_value = "config.yaml")]
    config: String,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Number of worker threads
    #[arg(short, long, default_value = "4")]
    threads: usize,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new config file with default rules
    Init {
        /// Create config in the current directory instead of ~/.config/asimeow/
        #[arg(long)]
        local: bool,

        /// Path where to create the config file (overrides --local)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Print the current version
    Version,
    /// List exclusions in the specified path
    /// - If path is a folder and ends with /, shows all files highlighting the exclusions
    /// - If path is a file or folder without /, shows the specific file or folder status
    /// - If no path is specified, shows the content of the current folder
    List {
        /// Path to list exclusions for
        path: Option<String>,
    },
    /// Explicitly exclude a single file or folder from Time Machine backups
    Exclude {
        /// Path to exclude from Time Machine backups
        path: String,
    },
    /// Explicitly include a single file or folder in Time Machine backups (remove exclusion)
    Include {
        /// Path to include in Time Machine backups
        path: String,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands
    if let Some(command) = &args.command {
        match command {
            Commands::Init { local, path } => {
                return config::create_default_config(*local, path.as_deref());
            }
            Commands::Version => {
                println!("Asimeow version {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Commands::List { path } => {
                return explorer::list_exclusions(path.as_deref());
            }
            Commands::Exclude { path } => {
                return explorer::exclude_path(path, args.verbose);
            }
            Commands::Include { path } => {
                return explorer::include_path(path, args.verbose);
            }
        }
    }

    if args.verbose {
        println!("Asimeow - Time Machine Exclusion Tool");
        println!("------------------------------------");
        println!("Using {} worker threads", args.threads);
    }

    // Load the configuration
    // If -c/--config is specified, use that path; otherwise, find the config automatically
    let config_path = if args.config != "config.yaml" {
        Some(args.config.as_str())
    } else {
        None
    };

    let (config, _) = config::load_config(config_path, args.verbose)?;

    // Run the explorer with the loaded configuration
    explorer::run_explorer(config, args.threads, args.verbose)?;

    Ok(())
}
