use anyhow::Result;
use asimeow::rules::Matcher;
use asimeow::{config, explorer, tm};
use std::fs::{self, File};
use std::sync::atomic::Ordering;
use tempfile::tempdir;

#[test]
fn test_exclusion_logic() -> Result<()> {
    // Create a temporary directory for our test
    let temp_dir = tempdir()?;
    let project_dir = temp_dir.path().join("test-exclusion");
    fs::create_dir_all(&project_dir)?;

    // Create a test config with specific rules
    let config = config::Config {
        roots: vec![config::Root {
            path: project_dir.to_str().unwrap().to_string(),
        }],
        ignore: vec![".git".to_string(), ".DS_Store".to_string()],
        rules: vec![
            config::Rule {
                name: "node".to_string(),
                file_match: "package.json".to_string(),
                exclusions: vec!["node_modules".to_string(), "dist".to_string()],
            },
            config::Rule {
                name: "rust".to_string(),
                file_match: "Cargo.toml".to_string(),
                exclusions: vec!["target".to_string()],
            },
        ],
    };

    // Save the config
    let config_path = temp_dir.path().join("config.yaml");
    let config_yaml = serde_yaml::to_string(&config)?;
    fs::write(&config_path, config_yaml)?;

    // Create a Node.js project
    let node_project = project_dir.join("node-project");
    fs::create_dir_all(&node_project)?;
    File::create(node_project.join("package.json"))?;

    // Create a Rust project
    let rust_project = project_dir.join("rust-project");
    fs::create_dir_all(&rust_project)?;
    File::create(rust_project.join("Cargo.toml"))?;

    // Create a file that should be excluded
    fs::create_dir_all(node_project.join("node_modules"))?;
    File::create(node_project.join("node_modules").join("test-module"))?;

    // Create a file that should not be excluded
    fs::create_dir_all(node_project.join("src"))?;
    File::create(node_project.join("src").join("index.js"))?;

    // Load the config
    let (config, _) = config::load_config(Some(config_path.to_str().unwrap()), false)?;

    // Process only the root directory: it returns the subdirectories to walk next
    let state = explorer::State::new(false, true);
    let matcher = Matcher::new(config.rules, &config.ignore);
    let mut children = explorer::process_path(&project_dir, &state, &matcher);
    children.sort();

    assert_eq!(state.processed_paths.load(Ordering::Relaxed), 1);
    assert_eq!(
        children,
        vec![node_project.clone(), rust_project.clone()],
        "both projects are walked next"
    );

    // Processing the node project finds node_modules and does not walk into it
    let children = explorer::process_path(&node_project, &state, &matcher);
    assert_eq!(children, vec![node_project.join("src")]);
    assert_eq!(state.exclusions_found.load(Ordering::Relaxed), 1);

    Ok(())
}

/// A project with a node_modules folder, and the config that scans it.
fn node_project() -> Result<(tempfile::TempDir, config::Config, std::path::PathBuf)> {
    let temp_dir = tempdir()?;
    let project = temp_dir.path().join("app");
    fs::create_dir_all(project.join("node_modules"))?;
    File::create(project.join("package.json"))?;
    let config = config::Config {
        roots: vec![config::Root {
            path: temp_dir.path().to_str().unwrap().to_string(),
        }],
        ignore: vec![],
        rules: vec![config::Rule {
            name: "node".to_string(),
            file_match: "package.json".to_string(),
            exclusions: vec!["node_modules".to_string()],
        }],
    };
    let node_modules = project.join("node_modules");
    Ok((temp_dir, config, node_modules))
}

#[test]
fn test_dry_run_writes_nothing() -> Result<()> {
    let (_temp_dir, config, node_modules) = node_project()?;

    let stats = explorer::run_explorer_with_stats(config, 2, false, true)?;

    assert_eq!(stats.newly_excluded, 1, "reported as would-be new");
    assert!(!tm::has_exclusion_mark(&node_modules), "but not written");
    Ok(())
}

#[test]
fn test_run_writes_the_exclusion_mark_once() -> Result<()> {
    let (_temp_dir, config, node_modules) = node_project()?;

    let first = explorer::run_explorer_with_stats(config.clone(), 2, false, false)?;
    assert_eq!(first.newly_excluded, 1);
    assert!(tm::has_exclusion_mark(&node_modules));
    // The mark is the one `tmutil addexclusion` writes
    let mark = std::process::Command::new("xattr")
        .args(["-p", tm::EXCLUSION_XATTR])
        .arg(&node_modules)
        .output()?;
    assert!(String::from_utf8_lossy(&mark.stdout).contains("com.apple.backupd"));

    let second = explorer::run_explorer_with_stats(config, 2, false, false)?;
    assert_eq!(second.exclusions_found, 1);
    assert_eq!(second.newly_excluded, 0, "already marked on the second run");
    Ok(())
}

#[test]
fn test_set_exclusion_mark_round_trip() -> Result<()> {
    let temp_dir = tempdir()?;
    let dir = temp_dir.path().join("build");
    fs::create_dir(&dir)?;

    assert!(tm::set_exclusion_mark(&dir, true));
    assert!(tm::has_exclusion_mark(&dir));
    assert!(tm::set_exclusion_mark(&dir, false));
    assert!(!tm::has_exclusion_mark(&dir));
    Ok(())
}
