use anyhow::{Context, Result};
use clap::Parser;
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

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

    // Process each root path
    for root in &config.roots {
        let expanded_path = expand_tilde(&root.path)?;

        if args.verbose {
            println!("Processing: {}", expanded_path.display());
        }

        if !expanded_path.exists() {
            eprintln!("Error: Path does not exist: {}", expanded_path.display());
            continue;
        }

        if !expanded_path.is_dir() {
            eprintln!("Error: Not a directory: {}", expanded_path.display());
            continue;
        }

        match process_directory(&expanded_path, &config.rules, args.verbose) {
            Ok(_) => {},
            Err(e) => eprintln!("Error: {}", e),
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

fn should_skip_dir(entry: &DirEntry, rules: &[(String, Pattern, Vec<String>)]) -> bool {
    let path = entry.path();

    // Only check directories
    if !entry.file_type().is_dir() {
        return false;
    }

    let dir_name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    let parent = match path.parent() {
        Some(p) => p,
        None => return false,
    };

    // Check if this directory is in the exclusion list of any rule
    // where the parent directory contains a matching file
    for (_, pattern, exclusions) in rules {
        // Skip if exclusions list is empty
        if exclusions.is_empty() {
            continue;
        }

        // Check if parent directory contains any file matching the rule
        let has_matching_file = fs::read_dir(parent)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|e| {
                        let name = e.file_name().to_string_lossy().to_lowercase();
                        pattern.matches(&name)
                    })
            })
            .unwrap_or(false);

        // Convert exclusions to lowercase for case-insensitive comparison
        let exclusions_lower: Vec<String> = exclusions.iter()
            .map(|ex| ex.to_lowercase())
            .collect();

        if has_matching_file && exclusions_lower.iter().any(|ex| ex == &dir_name) {
            return true;
        }
    }

    false
}

fn process_directory(root: &Path, rules: &[Rule], verbose: bool) -> Result<()> {
    // Convert rules to patterns for faster matching
    let rule_patterns: Vec<(String, Pattern, Vec<String>)> = rules
        .iter()
        .map(|rule| {
            let pattern = Pattern::new(&rule.file_match.to_lowercase())
                .with_context(|| format!("Invalid pattern in rule '{}': {}", rule.name, rule.file_match))
                .unwrap_or_else(|_| {
                    if verbose {
                        eprintln!("Warning: Invalid pattern '{}' in rule '{}', using literal match",
                                 rule.file_match, rule.name);
                    }
                    Pattern::new(&glob::Pattern::escape(&rule.file_match.to_lowercase())).unwrap()
                });

            (
                rule.name.clone(),
                pattern,
                rule.exclusions.clone(),
            )
        })
        .collect();

    // Track directories where we've already processed exclusions
    let mut processed_dirs = HashSet::new();

    let walker = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| !should_skip_dir(e, &rule_patterns));

    for entry_result in walker {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(err) => {
                if verbose {
                    eprintln!("Error accessing entry: {}", err);
                }
                continue;
            }
        };

        let path = entry.path();
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();

        // Check if this entry matches any rule
        for (rule_name, pattern, exclusions) in &rule_patterns {
            if pattern.matches(&file_name) {
                let dir_path = if path.is_file() {
                    path.parent().unwrap_or(path)
                } else {
                    path
                };

                // Only process each directory once per rule
                let key = (dir_path.to_path_buf(), rule_name.clone());
                if processed_dirs.contains(&key) {
                    continue;
                }

                processed_dirs.insert(key);

                if verbose {
                    println!("Found match for rule '{}' at: {}", rule_name, path.display());
                }

                // Check for and print exclusion paths that exist
                if !exclusions.is_empty() {
                    for exclusion in exclusions {
                        let exclusion_path = dir_path.join(exclusion);
                        if exclusion_path.exists() {
                            // Print in the requested format: /path/to/excluded/dir - rule-name
                            println!("{} - {}", exclusion_path.display(), rule_name);
                        } else if verbose {
                            println!("  Exclusion not found: {}", exclusion);
                        }
                    }
                } else if verbose {
                    println!("  No exclusions defined for this rule");
                }
            }
        }
    }

    Ok(())
}
