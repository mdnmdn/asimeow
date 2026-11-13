use anyhow::Result;
use asimeow::scan::{self, Event, Kind, Options, Progress, Summary};
use asimeow::{config, tm};
use std::fs::{self, File};
use std::sync::mpsc;
use tempfile::tempdir;

/// Runs a scan and collects everything it reported.
fn scan_all(config: config::Config, dry_run: bool) -> Result<(Summary, Vec<Event>)> {
    let (tx, rx) = mpsc::channel();
    let opts = Options {
        threads: 2,
        verbose: false,
        dry_run,
    };
    let summary = scan::scan(config, &opts, tx, &Progress::default())?;
    Ok((summary, rx.into_iter().collect()))
}

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

    // Scan: the root, both projects and node-project/src are walked; node_modules is
    // reported but not walked into
    let (summary, events) = scan_all(config, true)?;

    assert_eq!(summary.dirs, 4);
    assert_eq!(summary.exclusions, 1);
    assert_eq!(
        events,
        vec![Event::Exclusion {
            path: node_project.join("node_modules"),
            rule: "node".to_string(),
            kind: Kind::New,
        }]
    );
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

    let (stats, _) = scan_all(config, true)?;

    assert_eq!(stats.newly_excluded, 1, "reported as would-be new");
    assert!(!tm::has_exclusion_mark(&node_modules), "but not written");
    Ok(())
}

#[test]
fn test_run_writes_the_exclusion_mark_once() -> Result<()> {
    let (_temp_dir, config, node_modules) = node_project()?;

    let (first, _) = scan_all(config.clone(), false)?;
    assert_eq!(first.newly_excluded, 1);
    assert!(tm::has_exclusion_mark(&node_modules));
    // The mark is the one `tmutil addexclusion` writes
    let mark = std::process::Command::new("xattr")
        .args(["-p", tm::EXCLUSION_XATTR])
        .arg(&node_modules)
        .output()?;
    assert!(String::from_utf8_lossy(&mark.stdout).contains("com.apple.backupd"));

    let (second, events) = scan_all(config, false)?;
    assert_eq!(second.exclusions, 1);
    assert_eq!(second.newly_excluded, 0, "already marked on the second run");
    assert!(matches!(
        events.as_slice(),
        [Event::Exclusion {
            kind: Kind::Already,
            ..
        }]
    ));
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
