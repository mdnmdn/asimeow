use anyhow::{Context, Result};
use clap::Parser;
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread;

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
    exclusion_found: RwLock<i32>,
    processed_paths: RwLock<i32>,
    active_tasks: RwLock<usize>,
    processing_complete: RwLock<bool>,
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

    /// Number of worker threads
    #[arg(short, long, default_value = "4")]
    threads: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        println!("Asimaw - Folder Analysis Tool");
        println!("-----------------------------");
        println!("Reading config from: {}", args.config);
        println!("Using {} worker threads", args.threads);
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
        exclusion_found: RwLock::new(0),
        processed_paths: RwLock::new(0),
        active_tasks: RwLock::new(0),
        processing_complete: RwLock::new(false),
    });

    // We'll spawn threads directly instead of using a thread pool

    // Process each root path and add to initial queue
    for root in &config.roots {
        let expanded_path = expand_tilde(&root.path)?;

        // Add root paths to the queue
        let mut queue = state.folder_queue.write().unwrap();
        queue.push(expanded_path);
    }

    // Create Arc-wrapped rules for sharing
    let rules = Arc::new(config.rules);

    // Spawn worker threads to process the queue
    for _ in 0..args.threads {
        let state_clone = Arc::clone(&state);
        let rules_clone = Arc::clone(&rules);
        let verbose_clone = args.verbose;

        thread::spawn(move || {
            loop {
                // Check if processing is complete
                if *state_clone.processing_complete.read().unwrap() {
                    break;
                }

                // Try to get a path from the queue
                let next_path_option = {
                    let mut queue = state_clone.folder_queue.write().unwrap();
                    if !queue.is_empty() {
                        // Increment active tasks counter
                        let mut active = state_clone.active_tasks.write().unwrap();
                        *active += 1;

                        Some(queue.remove(0))
                    } else {
                        None
                    }
                };

                if let Some(next_path) = next_path_option {
                    // Process the path
                    if let Err(e) = process_path(&next_path, Arc::clone(&state_clone), &rules_clone, verbose_clone) {
                        eprintln!("Error processing path {}: {}", next_path.display(), e);
                    }

                    // Decrement active tasks counter
                    let mut active = state_clone.active_tasks.write().unwrap();
                    *active -= 1;
                } else {
                    // No paths in queue, check if we're done
                    let active_count = *state_clone.active_tasks.read().unwrap();
                    let queue_empty = state_clone.folder_queue.read().unwrap().is_empty();

                    if queue_empty && active_count == 0 {
                        // No more work to do, mark processing as complete
                        let mut complete = state_clone.processing_complete.write().unwrap();
                        *complete = true;
                        break;
                    }

                    // No work available right now, wait a bit
                    thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        });
    }

    // Wait for all processing to complete
    loop {
        let processing_done = *state.processing_complete.read().unwrap();
        if processing_done {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }

    // Print the total number of exclusions found and processed paths
    let exclusions_count = *state.exclusion_found.read().unwrap();
    let processed_count = *state.processed_paths.read().unwrap();

    if args.verbose || exclusions_count > 0 {
        println!("\nTotal paths processed: {}", processed_count);
        println!("Total exclusions found: {}", exclusions_count);
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

fn process_exclusion(path: &Path, rule: &Rule, state: &Arc<State>) {
    // Print in the requested format: /path/to/excluded/dir - rule-name
    for exclusion in &rule.exclusions {
        let exclusion_path = path.join(exclusion);
        if exclusion_path.exists() {
            println!("{} - {}", exclusion_path.display(), rule.name);

            // Increment the exclusion_found counter
            let mut counter = state.exclusion_found.write().unwrap();
            *counter += 1;
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

    // Increment the processed_paths counter
    {
        let mut counter = state.processed_paths.write().unwrap();
        *counter += 1;
    }

    if verbose {
        println!("Processing path: {}", path.display());
    }

    // Check if the current directory contains files matching any rule
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Failed to read directory {}: {}", path.display(), e);
            return Ok(());
        }
    };

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
                process_exclusion(path, rule, &state);
                break; // Found a match for this entry, no need to check other rules
            }
        }

        // If it's a directory, collect it for potential queue addition
        if entry_path.is_dir() {
            subdirs.push(entry_path);
        }
    }

    // Add subdirectories to the queue
    if !subdirs.is_empty() {
        let mut queue = state.folder_queue.write().unwrap();
        for subdir in subdirs {
            queue.push(subdir);
        }
    }

    Ok(())
}


