use crate::config::Rule;
use anyhow::Result;
use glob::Pattern;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::string::ToString;
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;

pub struct State {
    pub folder_queue: RwLock<Vec<PathBuf>>,
    pub exclusion_found: RwLock<i32>,
    pub processed_paths: RwLock<i32>,
    pub active_tasks: RwLock<usize>,
    pub processing_complete: RwLock<bool>,
    pub newly_excluded: RwLock<i32>,
}


static THIS_FOLDER: OnceLock<String> = OnceLock::new();
static PARENT_FOLDER: OnceLock<String> = OnceLock::new();

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        State {
            folder_queue: RwLock::new(Vec::new()),
            exclusion_found: RwLock::new(0),
            processed_paths: RwLock::new(0),
            active_tasks: RwLock::new(0),
            processing_complete: RwLock::new(false),
            newly_excluded: RwLock::new(0),
        }
    }
}

/// Excludes a path from Time Machine backups on macOS.
/// Returns true if the path was successfully excluded or false if it was already excluded.
pub fn exclude_from_timemachine(path: &Path) -> bool {
    // Check if the path is already excluded
    let check_output = Command::new("tmutil")
        .args(["isexcluded", path.to_str().unwrap_or_default()])
        .output();

    match check_output {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);

            // If the path is already excluded, tmutil will report "[Excluded]"
            if output_str.contains("[Excluded]") {
                return false; // Already excluded
            }

            // Exclude the path
            let exclude_result = Command::new("tmutil")
                .args(["addexclusion", path.to_str().unwrap_or_default()])
                .status();

            match exclude_result {
                Ok(status) => status.success(),
                Err(_) => false,
            }
        }
        Err(_) => false, // Failed to run tmutil
    }
}

fn process_exclusion(path: &Path, rule: &Rule, state: &Arc<State>, verbose: bool) {
    // Print in the requested format: /path/to/excluded/dir - rule-name
    for exclusion in &rule.exclusions {
        let exclusion_path = path.join(exclusion);
        if exclusion_path.exists() {
            // Try to exclude from Time Machine
            let excluded = exclude_from_timemachine(&exclusion_path);

            if excluded {
                // Green tick for newly excluded paths
                println!("✅ {} - {}", exclusion_path.display(), rule.name);

                // Increment the newly_excluded counter
                let mut newly_excluded = state.newly_excluded.write().unwrap();
                *newly_excluded += 1;

                if verbose {
                    println!(
                        "  → Excluded from Time Machine: {}",
                        exclusion_path.display()
                    );
                }
            } else {
                // Yellow circle for already excluded paths
                println!("🟡 {} - {}", exclusion_path.display(), rule.name);

                if verbose {
                    println!("  → Already excluded from Time Machine");
                }
            }

            // Increment the exclusion_found counter
            let mut counter = state.exclusion_found.write().unwrap();
            *counter += 1;
        }
    }
}

pub fn process_path(path: &Path, state: Arc<State>, rules: &[Rule], verbose: bool, ignore_patterns: &[String]) -> Result<()> {
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

    // Check if this directory should be ignored based on its name
    if let Some(dir_name) = path.file_name() {
        let dir_name_str = dir_name.to_string_lossy().to_string();
        for pattern in ignore_patterns {
            // Use glob pattern matching for ignore patterns
            let glob_pattern = match Pattern::new(pattern) {
                Ok(p) => p,
                Err(_) => {
                    if verbose {
                        eprintln!("Warning: Invalid ignore pattern '{}', using literal match", pattern);
                    }
                    Pattern::new(&glob::Pattern::escape(pattern)).unwrap()
                }
            };

            if glob_pattern.matches(&dir_name_str) {
                if verbose {
                    println!("Skipping ignored directory: {}", path.display());
                }
                return Ok(());
            }
        }
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

    let mut directory_to_ignore = vec![];

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
        let file_name = entry_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        // Check if this entry matches any rule
        for rule in rules {
            let pattern = match Pattern::new(&rule.file_match.to_lowercase()) {
                Ok(p) => p,
                Err(_) => {
                    if verbose {
                        eprintln!(
                            "Warning: Invalid pattern '{}' in rule '{}', using literal match",
                            rule.file_match, rule.name
                        );
                    }
                    Pattern::new(&glob::Pattern::escape(&rule.file_match.to_lowercase())).unwrap()
                }
            };

            if pattern.matches(&file_name) {
                if verbose {
                    println!(
                        "Found match for rule '{}' at: {}",
                        rule.name,
                        entry_path.display()
                    );
                }
                process_exclusion(path, rule, &state, verbose);
                // Return early if the rule has exclusions containing "." or ".."
                if rule.exclusions.contains(THIS_FOLDER.get_or_init(|| ".".to_string()))
                    || rule.exclusions.contains(PARENT_FOLDER.get_or_init(|| "..".to_string())) {
                    return Ok(())
                }
                rule.exclusions.iter().for_each(|exclusion| directory_to_ignore.push(exclusion.as_str()));

                break; // Found a match for this entry, no need to check other rules
            }
        }

        // If it's a directory, collect it for potential queue addition
        if entry_path.is_dir() {
            // Only add directories that are not explicitly ignored by their names
            if directory_to_ignore.is_empty() || !directory_to_ignore.contains(&entry_path.file_name().unwrap_or_default().to_string_lossy().as_ref()) {
                subdirs.push(entry_path);
            }

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

pub fn run_workers(
    state: Arc<State>,
    rules: Arc<Vec<Rule>>,
    thread_count: usize,
    verbose: bool,
    ignore_patterns: Arc<Vec<String>>,
) -> Result<()> {
    // Spawn worker threads to process the queue
    for _ in 0..thread_count {
        let state_clone = Arc::clone(&state);
        let rules_clone = Arc::clone(&rules);
        let ignore_patterns_clone = Arc::clone(&ignore_patterns);
        let verbose_clone = verbose;

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
                    if let Err(e) = process_path(
                        &next_path,
                        Arc::clone(&state_clone),
                        &rules_clone,
                        verbose_clone,
                        &ignore_patterns_clone,
                    ) {
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

    Ok(())
}

pub fn run_explorer(config: crate::config::Config, thread_count: usize, verbose: bool) -> Result<()> {
    // Create shared state
    let state = Arc::new(State::new());

    // Process each root path and add to initial queue
    for root in &config.roots {
        let expanded_path = crate::config::expand_tilde(&root.path)?;

        // Add root paths to the queue
        let mut queue = state.folder_queue.write().unwrap();
        queue.push(expanded_path);
    }

    // Create Arc-wrapped rules and ignore patterns for sharing
    let rules = Arc::new(config.rules);
    let ignore_patterns = Arc::new(config.ignore);

    // Run worker threads
    run_workers(state.clone(), rules, thread_count, verbose, ignore_patterns)?;

    // Print the total number of exclusions found and processed paths
    let exclusions_count = *state.exclusion_found.read().unwrap();
    let processed_count = *state.processed_paths.read().unwrap();
    let newly_excluded_count = *state.newly_excluded.read().unwrap();

    if verbose || exclusions_count > 0 {
        println!("\nTotal paths processed: {}", processed_count);
        println!("Total exclusions found: {}", exclusions_count);
        println!("Newly excluded from Time Machine: {}", newly_excluded_count);
    }

    Ok(())
}