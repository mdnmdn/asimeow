use anyhow::{Context, Result};
use clap::Parser;
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    roots: Vec<Root>,
    rules: Vec<Rule>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Root {
    path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Rule {
    name: String,
    file_match: String,
    exclusions: Vec<String>,
}

struct State {
    folder_queue: RwLock<Vec<PathBuf>>,
}

#[derive(Parser, Debug)]
#[command(
    author = "Asimaw",
    version = "0.1.0",
    about = "Recursively analyzes folders according to rules defined in a configuration file",
    long_about = None
)]
struct Args {
    /// Path to the config file
    #[arg(short, long, default_value = "config.yaml")]
    config: String,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        println!("Asimaw - Folder Analysis Tool");
        println!("-----------------------------");
        println!("Reading config from: {}", args.config);
    }

    // Read and parse the config file
    let config_content = fs::read_to_string(&args.config)
        .with_context(|| format!("Failed to read config file: {}", args.config))?;

    let config: Config = serde_yaml::from_str(&config_content)
        .with_context(|| format!("Failed to parse config file: {}", args.config))?;

    if args.verbose {
        println!("\nLoaded {} rules:", config.rules.len());
        for rule in &config.rules {
            println!("  - {} (pattern: {}, exclusions: {})",
                     rule.name,
                     rule.file_match,
                     rule.exclusions.join(", "));
        }
        println!();
    }

    if config.roots.is_empty() {
        eprintln!("Error: No root paths defined in config file");
        return Ok(());
    }

    // Create shared state
    let state = Arc::new(State {
        folder_queue: RwLock::new(Vec::new()),
    });

    // Process each root path
    for root in &config.roots {
        let expanded_path = expand_tilde(&root.path)?;

        // Process the root path
        match process_path(&expanded_path, Arc::clone(&state), &config.rules, args.verbose) {
            Ok(_) => {},
            Err(e) => eprintln!("Error processing root path {}: {}", expanded_path.display(), e),
        }
    }

    // Process all paths in the queue
    loop {
        let next_path = {
            let mut queue = state.folder_queue.write().unwrap();
            if queue.is_empty() {
                break;
            }
            queue.remove(0)
        };

        match process_path(&next_path, Arc::clone(&state), &config.rules, args.verbose) {
            Ok(_) => {},
            Err(e) => eprintln!("Error processing path {}: {}", next_path.display(), e),
        }
    }

    Ok(())
}

fn expand_tilde(path: &str) -> Result<PathBuf> {
    if path.starts_with("~/") {
        let home_dir = dirs::home_dir()
            .context("Could not determine home directory")?;
        Ok(home_dir.join(&path[2..]))
    } else {
        Ok(PathBuf::from(path))
    }
}

fn process_exclusion(path: &Path, rule: &Rule) {
    // Print in the requested format: /path/to/excluded/dir - rule-name
    for exclusion in &rule.exclusions {
        let exclusion_path = path.join(exclusion);
        if exclusion_path.exists() {
            println!("{} - {}", exclusion_path.display(), rule.name);
        }
    }
}

fn process_path(path: &Path, state: Arc<State>, rules: &[Rule], verbose: bool) -> Result<()> {
    // Skip if path doesn't exist or is not a directory
    if !path.exists() {
        if verbose {
            eprintln!("Error: Path does not exist: {}", path.display());
        }
        return Ok(());
    }

    if !path.is_dir() {
        if verbose {
            eprintln!("Error: Not a directory: {}", path.display());
        }
        return Ok(());
    }

    if verbose {
        println!("Processing path: {}", path.display());
    }

    // Check if the current directory contains files matching any rule
    let entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory: {}", path.display()))?;

    let mut subdirs = Vec::new();

    // First pass: collect all entries and check for rule matches
    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                if verbose {
                    eprintln!("Error accessing entry: {}", err);
                }
                continue;
            }
        };

        let entry_path = entry.path();
        let file_name = entry_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();



        // Check if this entry matches any rule
        for rule in rules {
            let pattern = match Pattern::new(&rule.file_match.to_lowercase()) {
                Ok(p) => p,
                Err(_) => {
                    if verbose {
                        eprintln!("Warning: Invalid pattern '{}' in rule '{}', using literal match",
                                 rule.file_match, rule.name);
                    }
                    Pattern::new(&glob::Pattern::escape(&rule.file_match.to_lowercase())).unwrap()
                }
            };

            if pattern.matches(&file_name) {
                if verbose {
                    println!("Found match for rule '{}' at: {}", rule.name, entry_path.display());
                }
                // matched_rules.push(rule);
                process_exclusion(path, rule);
                continue;
            }
        }

        // If it's a directory, collect it for potential queue addition
        if entry_path.is_dir() {
            subdirs.push(entry_path);
        }
    }


    // Add subdirectories to the queue if no rules matched
    if !subdirs.is_empty() {
        let mut queue = state.folder_queue.write().unwrap();
        for subdir in subdirs {
            queue.push(subdir);
        }
    }

    Ok(())
}


