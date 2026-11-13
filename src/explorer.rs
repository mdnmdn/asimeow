//! Manual commands: list, exclude and include single paths.

use crate::tm;
use anyhow::Result;
use std::fs;

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
