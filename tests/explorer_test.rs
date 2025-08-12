use anyhow::Result;
use asimeow::{config, explorer};
use std::fs::{self, File};
use tempfile::tempdir;

fn create_test_project(project_name: &str, rules: Vec<config::Rule>) -> Result<tempfile::TempDir> {
    let temp_dir = tempdir()?;
    let project_dir = temp_dir.path().join(project_name);
    fs::create_dir_all(&project_dir)?;

    // Create a test config
    let config = config::Config {
        roots: vec![config::Root {
            path: project_dir.to_str().unwrap().to_string(),
        }],
        ignore: vec![".git".to_string(), ".DS_Store".to_string()],
        rules,
    };

    // Save the config to the temp dir for reference
    let config_path = temp_dir.path().join("config.yaml");
    let config_yaml = serde_yaml::to_string(&config)?;
    fs::write(&config_path, config_yaml)?;

    Ok(temp_dir)
}

#[test]
fn test_process_path_with_node_project() -> Result<()> {
    // Create a temporary directory for our test
    let temp_dir = create_test_project(
        "test-node-project",
        vec![config::Rule {
            name: "node".to_string(),
            file_match: "package.json".to_string(),
            exclusions: vec!["node_modules".to_string(), "dist".to_string()],
        }],
    )?;

    let project_dir = temp_dir.path().join("test-node-project");

    // Create package.json
    let package_json = project_dir.join("package.json");
    File::create(&package_json)?;

    // Create node_modules directory
    let node_modules = project_dir.join("node_modules");
    fs::create_dir(&node_modules)?;

    // Create dist directory (should also be excluded)
    let dist_dir = project_dir.join("dist");
    fs::create_dir(&dist_dir)?;

    // Create a test file in node_modules
    File::create(node_modules.join("test-module"))?;

    // Load the config we created
    let (config, _) = config::load_config(
        Some(temp_dir.path().join("config.yaml").to_str().unwrap()),
        false,
    )?;

    // Run the explorer
    let result = explorer::run_explorer(config, 1, false);

    // Verify it runs without errors
    assert!(result.is_ok());

    Ok(())
}

#[test]
fn test_does_not_enqueue_children_of_excluded_dir() -> Result<()> {
    // Arrange a project with node rule and a nested node_modules tree
    let temp_dir = create_test_project(
        "test-skip-excluded-children",
        vec![config::Rule {
            name: "node".to_string(),
            file_match: "package.json".to_string(),
            exclusions: vec!["node_modules".to_string()],
        }],
    )?;

    let project_dir = temp_dir.path().join("test-skip-excluded-children");

    // Root indicator + excluded dir
    File::create(project_dir.join("package.json"))?;
    let node_modules = project_dir.join("node_modules");
    fs::create_dir_all(&node_modules)?;

    // Create nested structure inside node_modules that would normally be traversed
    let nested = node_modules.join("a").join("b").join("c");
    fs::create_dir_all(&nested)?;
    File::create(nested.join("package.json"))?; // should be ignored because under excluded dir

    // Act
    let (cfg, _) = config::load_config(
        Some(temp_dir.path().join("config.yaml").to_str().unwrap()),
        false,
    )?;
    let stats = explorer::run_explorer_with_stats(cfg, 2, false)?;

    // Assert: we should process only the project root (and maybe a few siblings),
    // but never descend into node_modules. Since the traversal counts processed directories,
    // ensure it's small and not proportional to nested depth we created.
    // At minimum it must be >= 1 (the root). Cap at < 5 to indicate we didn't walk deep.
    assert!(stats.processed_paths >= 1);
    assert!(stats.processed_paths < 5);

    Ok(())
}

#[test]
fn test_ignore_patterns() -> Result<()> {
    // Create a temporary directory for our test
    let temp_dir = create_test_project(
        "test-project",
        vec![config::Rule {
            name: "node".to_string(),
            file_match: "package.json".to_string(),
            exclusions: vec!["node_modules".to_string()],
        }],
    )?;

    let project_dir = temp_dir.path().join("test-project");

    // Create a .git directory that should be ignored
    let git_dir = project_dir.join(".git");
    fs::create_dir(&git_dir)?;

    // Create a test file in .git
    File::create(git_dir.join("test"))?;

    // Create a package.json to trigger processing
    File::create(project_dir.join("package.json"))?;

    // Load the config we created
    let (config, _) = config::load_config(
        Some(temp_dir.path().join("config.yaml").to_str().unwrap()),
        false,
    )?;

    // Run the explorer
    let result = explorer::run_explorer(config, 1, false);

    // Should run without errors
    assert!(result.is_ok());

    Ok(())
}

#[test]
fn test_multiple_rules() -> Result<()> {
    // Create a project with multiple rules
    let temp_dir = create_test_project(
        "test-multi-project",
        vec![
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
    )?;

    let project_dir = temp_dir.path().join("test-multi-project");

    // Create a Node.js project
    File::create(project_dir.join("package.json"))?;
    fs::create_dir_all(project_dir.join("node_modules"))?;

    // Create a Rust project in a subdirectory
    let rust_project = project_dir.join("rust-lib");
    fs::create_dir_all(&rust_project)?;
    File::create(rust_project.join("Cargo.toml"))?;
    fs::create_dir_all(rust_project.join("target"))?;

    // Load the config
    let (config, _) = config::load_config(
        Some(temp_dir.path().join("config.yaml").to_str().unwrap()),
        false,
    )?;

    // Run the explorer
    let result = explorer::run_explorer(config, 2, false); // Use 2 threads
    assert!(result.is_ok());

    Ok(())
}

#[test]
fn test_nested_projects() -> Result<()> {
    // Create a project with nested projects
    let temp_dir = create_test_project(
        "test-nested-projects",
        vec![config::Rule {
            name: "node".to_string(),
            file_match: "package.json".to_string(),
            exclusions: vec!["node_modules".to_string()],
        }],
    )?;

    let project_dir = temp_dir.path().join("test-nested-projects");

    // Create a root package.json
    File::create(project_dir.join("package.json"))?;

    // Create a nested project
    let nested_dir = project_dir.join("nested");
    fs::create_dir(&nested_dir)?;
    File::create(nested_dir.join("package.json"))?;

    // Create a deeply nested project
    let deep_nested = nested_dir.join("deep");
    fs::create_dir_all(&deep_nested)?;
    File::create(deep_nested.join("package.json"))?;

    // Load the config
    let (config, _) = config::load_config(
        Some(temp_dir.path().join("config.yaml").to_str().unwrap()),
        false,
    )?;

    // Run the explorer
    let result = explorer::run_explorer(config, 1, false);
    assert!(result.is_ok());

    Ok(())
}
