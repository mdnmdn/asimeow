use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub roots: Vec<Root>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Root {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub file_match: String,
    pub exclusions: Vec<String>,
}

/// Creates a default config file with common development project rules
pub fn create_default_config(path: &str) -> Result<()> {
    // Create a default config with common rules
    let config = Config {
        roots: vec![Root {
            path: "~/".to_string(),
        }],
        rules: vec![
            Rule {
                name: "net".to_string(),
                file_match: "*.csproj".to_string(),
                exclusions: vec!["obj".to_string(), "bin".to_string(), "packages".to_string()],
            },
            Rule {
                name: "rust".to_string(),
                file_match: "cargo.toml".to_string(),
                exclusions: vec!["target".to_string()],
            },
            Rule {
                name: "go".to_string(),
                file_match: "go.mod".to_string(),
                exclusions: vec!["vendor".to_string()],
            },
            Rule {
                name: "node".to_string(),
                file_match: "package.json".to_string(),
                exclusions: vec!["node_modules".to_string(), "dist".to_string()],
            },
            Rule {
                name: "python".to_string(),
                file_match: "requirements.txt".to_string(),
                exclusions: vec!["__pycache__".to_string(), ".venv".to_string()],
            },
            Rule {
                name: "java".to_string(),
                file_match: "pom.xml".to_string(),
                exclusions: vec!["target".to_string()],
            },
            Rule {
                name: "php".to_string(),
                file_match: "composer.json".to_string(),
                exclusions: vec!["vendor".to_string()],
            },
            Rule {
                name: "vagrant".to_string(),
                file_match: "Vagrantfile".to_string(),
                exclusions: vec![".vagrant".to_string()],
            },
            Rule {
                name: "bower".to_string(),
                file_match: "bower.json".to_string(),
                exclusions: vec!["bower_components".to_string()],
            },
            Rule {
                name: "haskell".to_string(),
                file_match: "stack.yaml".to_string(),
                exclusions: vec![".stack-work".to_string()],
            },
            Rule {
                name: "carthage".to_string(),
                file_match: "Cartfile".to_string(),
                exclusions: vec!["Carthage".to_string()],
            },
            Rule {
                name: "cocoapods".to_string(),
                file_match: "Podfile".to_string(),
                exclusions: vec!["Pods".to_string()],
            },
            Rule {
                name: "swift".to_string(),
                file_match: "Package.swift".to_string(),
                exclusions: vec![".build".to_string()],
            },
            Rule {
                name: "elixir".to_string(),
                file_match: "mix.exs".to_string(),
                exclusions: vec!["_build".to_string()],
            },
            Rule {
                name: "project".to_string(),
                file_match: "*.prj".to_string(),
                exclusions: vec!["bin".to_string(), "debug".to_string()],
            },
        ],
    };

    // Serialize the config to YAML
    let yaml =
        serde_yaml::to_string(&config).context("Failed to serialize default config to YAML")?;

    // Check if the file already exists
    let path_obj = Path::new(path);
    if path_obj.exists() {
        return Err(anyhow::anyhow!("Config file already exists at: {}", path));
    }

    // Create the file and write the YAML content
    let mut file = fs::File::create(path)
        .with_context(|| format!("Failed to create config file at: {}", path))?;

    file.write_all(yaml.as_bytes())
        .with_context(|| format!("Failed to write to config file at: {}", path))?;

    println!("✅ Created default config file at: {}", path);
    println!("You may want to edit the file to customize the root paths for your system.");

    Ok(())
}

pub fn load_config(config_path: &str, verbose: bool) -> Result<Config> {
    if verbose {
        println!("Reading config from: {}", config_path);
    }

    // Read and parse the config file
    let config_content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path))?;

    let config: Config = serde_yaml::from_str(&config_content)
        .with_context(|| format!("Failed to parse config file: {}", config_path))?;

    if verbose {
        println!("\nLoaded {} rules:", config.rules.len());
        for rule in &config.rules {
            println!(
                "  - {} (pattern: {}, exclusions: {})",
                rule.name,
                rule.file_match,
                rule.exclusions.join(", ")
            );
        }
        println!();
    }

    if config.roots.is_empty() {
        return Err(anyhow::anyhow!("No root paths defined in config file"));
    }

    Ok(config)
}

pub fn expand_tilde(path: &str) -> Result<PathBuf> {
    if path.starts_with("~/") {
        let home_dir = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home_dir.join(&path[2..]))
    } else {
        Ok(PathBuf::from(path))
    }
}