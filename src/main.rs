use anyhow::Result;
use clap::{Parser, Subcommand};
use asimeow::config;
use asimeow::explorer;

#[derive(Parser, Debug)]
#[command(
    author = "mdnmdn",
    version = "0.1.0",
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
        /// Path where to create the config file
        #[arg(short, long)]
        path: Option<String>,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands
    if let Some(command) = &args.command {
        match command {
            Commands::Init { path } => {
                let config_path = path.as_deref().unwrap_or(&args.config);
                return config::create_default_config(config_path);
            }
        }
    }

    if args.verbose {
        println!("Asimeow - Time Machine Exclusion Tool");
        println!("------------------------------------");
        println!("Using {} worker threads", args.threads);
    }

    // Load the configuration
    let config = config::load_config(&args.config, args.verbose)?;

    // Run the explorer with the loaded configuration
    explorer::run_explorer(config, args.threads, args.verbose)?;

    Ok(())
}
