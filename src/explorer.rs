use crate::config::Rule;
use anyhow::Result;
use glob::Pattern;
use std::collections::{HashMap, HashSet};
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
    // Tracks exclusion paths we already attempted this run to avoid repeated tmutil calls
    pub seen_exclusion_paths: RwLock<HashSet<String>>,
    // Optional memoization for exclusion status checks (path -> is_excluded)
    pub exclusion_status_cache: RwLock<HashMap<String, bool>>,
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
            seen_exclusion_paths: RwLock::new(HashSet::new()),
            exclusion_status_cache: RwLock::new(HashMap::new()),
        }
    }
}

/// Checks if a path is excluded from Time Machine backups on macOS.
/// Returns true if the path is excluded, false otherwise.
pub fn is_excluded_from_timemachine(path: &Path) -> bool {
    let check_output = Command::new("tmutil")
        .args(["isexcluded", path.to_str().unwrap_or_default()])
        .output();

    match check_output {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            output_str.contains("[Excluded]")
        }
        Err(_) => false, // Failed to run tmutil
    }
}

/// Excludes a path from Time Machine backups on macOS.
/// Returns true if the path was successfully excluded or false if it was already excluded.
pub fn exclude_from_timemachine(path: &Path) -> bool {
    // Check if the path is already excluded
    if is_excluded_from_timemachine(path) {
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

/// Removes a path from Time Machine exclusions on macOS.
/// Returns true if the path was successfully included or false if it was already included.
pub fn include_in_timemachine(path: &Path) -> bool {
    // Check if the path is already included (not excluded)
    if !is_excluded_from_timemachine(path) {
        return false; // Already included
    }

    // Include the path (remove exclusion)
    let include_result = Command::new("tmutil")
        .args(["removeexclusion", path.to_str().unwrap_or_default()])
        .status();

    match include_result {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn process_exclusion(path: &Path, rule: &Rule, state: &Arc<State>, verbose: bool) {
    // Print in the requested format: /path/to/excluded/dir - rule-name
    for exclusion in &rule.exclusions {
        let exclusion_path = path.join(exclusion);
        if exclusion_path.exists() {
            // Skip if we already processed this exact exclusion path in this run
            let exclusion_str = exclusion_path.display().to_string();
            {
                let seen = state.seen_exclusion_paths.read().unwrap();
                if seen.contains(&exclusion_str) {
                    continue;
                }
            }

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

            // Mark as seen to avoid repeated tmutil calls on the same path
            let mut seen = state.seen_exclusion_paths.write().unwrap();
            seen.insert(exclusion_str);
        }
    }
}

pub fn process_path(
    path: &Path,
    state: Arc<State>,
    rules: &[Rule],
    verbose: bool,
    ignore_patterns: &[String],
) -> Result<()> {
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
                        eprintln!(
                            "Warning: Invalid ignore pattern '{}', using literal match",
                            pattern
                        );
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

    // Read all entries once
    let read_dir_iter = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Failed to read directory {}: {}", path.display(), e);
            return Ok(());
        }
    };

    // Collect entries into memory to ensure deterministic two-phase processing
    let mut entries: Vec<fs::DirEntry> = Vec::new();
    for entry_result in read_dir_iter {
        match entry_result {
            Ok(entry) => entries.push(entry),
            Err(err) => {
                if verbose {
                    eprintln!("Error accessing entry: {}", err);
                }
            }
        }
    }

    // Phase 1: evaluate rule matches and compute directories to ignore
    let mut directory_to_ignore: Vec<String> = Vec::new();
    for entry in &entries {
        let entry_path = entry.path();
        let file_name_lc = entry_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

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

            if pattern.matches(&file_name_lc) {
                if verbose {
                    println!(
                        "Found match for rule '{}' at: {}",
                        rule.name,
                        entry_path.display()
                    );
                }
                process_exclusion(path, rule, &state, verbose);

                // If special entries are present, do not descend further from current folder
                if rule
                    .exclusions
                    .contains(THIS_FOLDER.get_or_init(|| ".".to_string()))
                    || rule
                        .exclusions
                        .contains(PARENT_FOLDER.get_or_init(|| "..".to_string()))
                {
                    return Ok(());
                }

                for exclusion in &rule.exclusions {
                    directory_to_ignore.push(exclusion.clone());
                }

                break; // no need to check other rules for this same entry
            }
        }
    }

    // Phase 2: enqueue subdirectories excluding those we just excluded
    if !entries.is_empty() {
        let mut queue = state.folder_queue.write().unwrap();
        for entry in entries {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                let name = entry_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if directory_to_ignore.iter().any(|n| n == &name) {
                    continue;
                }

                queue.push(entry_path);
            }
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

/// Lists the exclusion status of files and directories in a given path
pub fn list_exclusions(path_str: Option<&str>) -> Result<()> {
    // If no path is provided, use the current directory
    let path = if let Some(p) = path_str {
        crate::config::expand_tilde(p)?
    } else {
        std::env::current_dir()?
    };

    if !path.exists() {
        return Err(anyhow::anyhow!("Path does not exist: {}", path.display()));
    }

    // Check if we're listing a directory with all contents or just a single file/directory
    let is_directory_listing = path.is_dir() && path_str.is_none_or(|p| p.ends_with('/'));

    if is_directory_listing {
        // List all entries in the directory
        println!("Listing contents of: {}", path.display());
        println!("------------------------------------");

        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) => return Err(anyhow::anyhow!("Failed to read directory: {}", e)),
        };

        let mut has_entries = false;
        for entry_result in entries {
            has_entries = true;
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("Error accessing entry: {}", e);
                    continue;
                }
            };

            let entry_path = entry.path();
            let is_excluded = is_excluded_from_timemachine(&entry_path);

            // Format the output with appropriate indicators
            let indicator = if is_excluded { "🟡" } else { "  " };
            let type_indicator = if entry_path.is_dir() { "/" } else { "" };

            println!(
                "{} {}{}",
                indicator,
                entry_path.file_name().unwrap_or_default().to_string_lossy(),
                type_indicator
            );
        }

        if !has_entries {
            println!("  (empty directory)");
        }

        // Add a legend
        println!("\nLegend:");
        println!("🟡 - Excluded from Time Machine");
        println!("  - Included in Time Machine");
        println!("/ - Directory");
    } else {
        // Just check the status of the specific path but format it like the directory listing
        let item_type = if path.is_dir() { "directory" } else { "file" };
        println!("Status of {}: {}", item_type, path.display());
        println!("------------------------------------");

        let is_excluded = is_excluded_from_timemachine(&path);
        let indicator = if is_excluded { "🟡" } else { "  " };
        let type_indicator = if path.is_dir() { "/" } else { "" };

        // Use the filename if available, otherwise use the full path
        let display_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        println!("{} {}{}", indicator, display_name, type_indicator);

        // Add a legend
        println!("\nLegend:");
        println!("🟡 - Excluded from Time Machine");
        println!("  - Included in Time Machine");
        if path.is_dir() {
            println!("/ - Directory");
        }
    }

    Ok(())
}

/// Explicitly excludes a single file or folder from Time Machine backups
pub fn exclude_path(path_str: &str, verbose: bool) -> Result<()> {
    // Expand the path if it contains a tilde
    let path = crate::config::expand_tilde(path_str)?;

    if !path.exists() {
        return Err(anyhow::anyhow!("Path does not exist: {}", path.display()));
    }

    let item_type = if path.is_dir() { "directory" } else { "file" };

    if verbose {
        println!(
            "Excluding {} from Time Machine: {}",
            item_type,
            path.display()
        );
    }

    let excluded = exclude_from_timemachine(&path);

    if excluded {
        println!("✅ Successfully excluded: {}", path.display());
    } else {
        println!("🟡 Already excluded: {}", path.display());
    }

    Ok(())
}

/// Explicitly includes a single file or folder in Time Machine backups (removes exclusion)
pub fn include_path(path_str: &str, verbose: bool) -> Result<()> {
    // Expand the path if it contains a tilde
    let path = crate::config::expand_tilde(path_str)?;

    if !path.exists() {
        return Err(anyhow::anyhow!("Path does not exist: {}", path.display()));
    }

    let item_type = if path.is_dir() { "directory" } else { "file" };

    if verbose {
        println!(
            "Including {} in Time Machine: {}",
            item_type,
            path.display()
        );
    }

    let included = include_in_timemachine(&path);

    if included {
        println!("✅ Successfully included: {}", path.display());
    } else {
        println!("  Already included: {}", path.display());
    }

    Ok(())
}

pub fn run_explorer(
    config: crate::config::Config,
    thread_count: usize,
    verbose: bool,
) -> Result<()> {
    let _ = run_explorer_with_stats(config, thread_count, verbose)?;
    Ok(())
}

pub struct ExplorerStats {
    pub processed_paths: i32,
    pub exclusions_found: i32,
    pub newly_excluded: i32,
}

/// Same as run_explorer but returns stats for testing/inspection
pub fn run_explorer_with_stats(
    config: crate::config::Config,
    thread_count: usize,
    verbose: bool,
) -> Result<ExplorerStats> {
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

    // Gather stats
    let exclusions_count = *state.exclusion_found.read().unwrap();
    let processed_count = *state.processed_paths.read().unwrap();
    let newly_excluded_count = *state.newly_excluded.read().unwrap();

    if verbose || exclusions_count > 0 {
        println!("\nTotal paths processed: {}", processed_count);
        println!("Total exclusions found: {}", exclusions_count);
        println!("Newly excluded from Time Machine: {}", newly_excluded_count);
    }

    Ok(ExplorerStats {
        processed_paths: processed_count,
        exclusions_found: exclusions_count,
        newly_excluded: newly_excluded_count,
    })
}
