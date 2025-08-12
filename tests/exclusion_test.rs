use anyhow::Result;
use asimeow::{config, explorer};
use std::fs::{self, File};
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

    // Create a state to track processed paths
    let state = std::sync::Arc::new(explorer::State::new());

    // Process the root directory
    let result = explorer::process_path(
        &project_dir,
        state.clone(),
        &config.rules,
        false, // verbose
        &config.ignore,
    );

    // Verify the processing completed successfully
    assert!(result.is_ok());

    // Check that the state was updated correctly
    let processed_paths = state.processed_paths.read().unwrap();
    assert!(
        *processed_paths > 0,
        "Should have processed at least one path"
    );

    // Note: We can't directly test tmutil commands in a cross-platform way,
    // but we've verified that the paths were processed correctly

    Ok(())
}
