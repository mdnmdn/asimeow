use crate::config::Rule;
use crate::rules::Matcher;
use crate::tm;
use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;

/// Settings and counters shared by the worker threads.
#[derive(Default)]
pub struct State {
    pub verbose: bool,
    /// Report new exclusions without writing them.
    pub dry_run: bool,
    pub processed_paths: AtomicUsize,
    pub exclusions_found: AtomicUsize,
    pub newly_excluded: AtomicUsize,
}

impl State {
    pub fn new(verbose: bool, dry_run: bool) -> Self {
        State {
            verbose,
            dry_run,
            ..Self::default()
        }
    }
}

/// Work queue for the walk. `pending` counts directories that are queued or being
/// processed, and both live under one lock, so "no work left" can't be observed while
/// another worker is between taking a directory and queueing its children.
struct Queue {
    inner: Mutex<(VecDeque<PathBuf>, usize)>,
    changed: Condvar,
}

impl Queue {
    fn new(roots: Vec<PathBuf>) -> Self {
        let pending = roots.len();
        Queue {
            inner: Mutex::new((roots.into(), pending)),
            changed: Condvar::new(),
        }
    }

    /// Next directory to process, or None once the whole tree is done.
    fn pop(&self) -> Option<PathBuf> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            if let Some(path) = inner.0.pop_front() {
                return Some(path);
            }
            if inner.1 == 0 {
                return None;
            }
            inner = self.changed.wait(inner).unwrap();
        }
    }

    /// Marks one directory as done and queues its subdirectories.
    fn finish(&self, children: Vec<PathBuf>) {
        let mut inner = self.inner.lock().unwrap();
        inner.1 = inner.1 + children.len() - 1;
        inner.0.extend(children);
        self.changed.notify_all();
    }
}

/// Excludes each existing exclusion folder of `rule` inside `dir`, skipping names already
/// handled for this directory (two rules can list the same folder).
fn process_exclusion(dir: &Path, rule: &Rule, seen: &mut HashSet<String>, state: &State) {
    for exclusion in &rule.exclusions {
        if !seen.insert(exclusion.clone()) {
            continue;
        }
        let exclusion_path = dir.join(exclusion);
        if !exclusion_path.exists() {
            continue;
        }
        state.exclusions_found.fetch_add(1, Ordering::Relaxed);

        if tm::has_exclusion_mark(&exclusion_path) {
            // Yellow circle for already excluded paths
            println!("🟡 {} - {}", exclusion_path.display(), rule.name);
            if state.verbose {
                println!("  → Already excluded from Time Machine");
            }
        } else if state.dry_run {
            state.newly_excluded.fetch_add(1, Ordering::Relaxed);
            println!("✅ {} - {} (dry run)", exclusion_path.display(), rule.name);
        } else if tm::set_exclusion_mark(&exclusion_path, true) {
            state.newly_excluded.fetch_add(1, Ordering::Relaxed);
            // Green tick for newly excluded paths
            println!("✅ {} - {}", exclusion_path.display(), rule.name);
            if state.verbose {
                println!(
                    "  → Excluded from Time Machine: {}",
                    exclusion_path.display()
                );
            }
        } else {
            eprintln!(
                "❌ {} - {} (could not be excluded)",
                exclusion_path.display(),
                rule.name
            );
        }
    }
}

/// Processes one directory: matches its entries against the rules, excludes what they
/// name, and returns the subdirectories to walk next. Symlinks are never followed.
pub fn process_path(path: &Path, state: &State, matcher: &Matcher) -> Vec<PathBuf> {
    let verbose = state.verbose;
    if let Some(name) = path.file_name() {
        if matcher.is_ignored(&name.to_string_lossy()) {
            if verbose {
                println!("Skipping ignored directory: {}", path.display());
            }
            return Vec::new();
        }
    }

    state.processed_paths.fetch_add(1, Ordering::Relaxed);
    if verbose {
        println!("Processing path: {}", path.display());
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Failed to read directory {}: {}", path.display(), e);
            return Vec::new();
        }
    };

    // Phase 1: match every entry against the rules and collect the subdirectories.
    let mut matched: Vec<&Rule> = Vec::new();
    let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                if verbose {
                    eprintln!("Error accessing entry: {}", err);
                }
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(rule) = matcher.rule_for(&name) {
            if verbose {
                println!(
                    "Found match for rule '{}' at: {}",
                    rule.name,
                    entry.path().display()
                );
            }
            matched.push(rule);
        }
        // file_type() comes from readdir and does not follow symlinks.
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            subdirs.push((name, entry.path()));
        }
    }

    // Phase 2: exclude what the rules name; never descend into excluded folders.
    let mut excluded_names: HashSet<String> = HashSet::new();
    for rule in matched {
        process_exclusion(path, rule, &mut excluded_names, state);
        // A rule that excludes "." or ".." stops the walk below this directory.
        if rule.exclusions.iter().any(|e| e == "." || e == "..") {
            return Vec::new();
        }
    }

    subdirs
        .into_iter()
        .filter(|(name, _)| !excluded_names.contains(name))
        .map(|(_, path)| path)
        .collect()
}

/// Walks all roots with `thread_count` workers until the whole tree is processed.
pub fn run_workers(roots: Vec<PathBuf>, state: &State, matcher: &Matcher, thread_count: usize) {
    let queue = Queue::new(roots);
    thread::scope(|scope| {
        for _ in 0..thread_count.max(1) {
            scope.spawn(|| {
                while let Some(dir) = queue.pop() {
                    let children = process_path(&dir, state, matcher);
                    queue.finish(children);
                }
            });
        }
    });
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
            let is_excluded = tm::is_excluded(&entry_path);

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

        let is_excluded = tm::is_excluded(&path);
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

    if tm::is_excluded(&path) {
        println!("🟡 Already excluded: {}", path.display());
    } else if tm::set_exclusion_mark(&path, true) {
        println!("✅ Successfully excluded: {}", path.display());
    } else {
        return Err(anyhow::anyhow!("Could not exclude: {}", path.display()));
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

    if !tm::is_excluded(&path) {
        println!("  Already included: {}", path.display());
    } else if tm::set_exclusion_mark(&path, false) && !tm::is_excluded(&path) {
        println!("✅ Successfully included: {}", path.display());
    } else {
        println!(
            "🟡 Still excluded by a parent folder or a system rule: {}",
            path.display()
        );
    }

    Ok(())
}

pub fn run_explorer(
    config: crate::config::Config,
    thread_count: usize,
    verbose: bool,
    dry_run: bool,
) -> Result<()> {
    let _ = run_explorer_with_stats(config, thread_count, verbose, dry_run)?;
    Ok(())
}

pub struct ExplorerStats {
    pub processed_paths: usize,
    pub exclusions_found: usize,
    pub newly_excluded: usize,
}

/// Same as run_explorer but returns stats for testing/inspection.
/// With `dry_run`, nothing is written: new exclusions are only reported.
pub fn run_explorer_with_stats(
    config: crate::config::Config,
    thread_count: usize,
    verbose: bool,
    dry_run: bool,
) -> Result<ExplorerStats> {
    let mut roots = Vec::new();
    for root in &config.roots {
        let path = crate::config::expand_tilde(&root.path)?;
        if path.is_dir() {
            roots.push(path);
        } else {
            eprintln!("Warning: root is not a directory: {}", path.display());
        }
    }

    let matcher = Matcher::new(config.rules, &config.ignore);
    let state = State::new(verbose, dry_run);
    run_workers(roots, &state, &matcher, thread_count);

    let stats = ExplorerStats {
        processed_paths: state.processed_paths.into_inner(),
        exclusions_found: state.exclusions_found.into_inner(),
        newly_excluded: state.newly_excluded.into_inner(),
    };

    if verbose || stats.exclusions_found > 0 {
        println!("\nTotal paths processed: {}", stats.processed_paths);
        println!("Total exclusions found: {}", stats.exclusions_found);
        let dry = if dry_run { " (dry run)" } else { "" };
        println!(
            "Newly excluded from Time Machine: {}{}",
            stats.newly_excluded, dry
        );
    }

    Ok(stats)
}
