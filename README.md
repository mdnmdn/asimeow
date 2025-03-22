# Asimeow

[![Rust CI/CD](https://github.com/mdnmdn/asimeow/actions/workflows/rust.yml/badge.svg)](https://github.com/mdnmdn/asimeow/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/asimeow.svg)](https://crates.io/crates/asimeow)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A command-line tool that automatically manages macOS Time Machine exclusions for developer projects. It recursively analyzes folders according to rules defined in a configuration file and excludes development artifacts from Time Machine backups.

## Features

- Recursively explores directories from specified root paths
- Identifies files matching patterns defined in rules (like package.json, cargo.toml, etc.)
- Automatically excludes development artifacts from Time Machine backups
- Shows which directories were newly excluded vs. already excluded
- Multi-threaded for fast processing of large directory structures
- Skips exploring excluded directories

## Installation

### From crates.io

```bash
cargo install asimeow
```

### From Source

```bash
git clone https://github.com/mdnmdn/asimeow.git
cd asimeow
cargo build --release
```

The executable will be available at `target/release/asimeow`.

## Usage

```bash
# Run with automatic config file detection
./asimeow

# Specify a custom config file
./asimeow -c /path/to/config.yaml

# Enable verbose output
./asimeow -v

# Specify number of worker threads (default: 4)
./asimeow -t 8

# Create a default configuration file in ~/.config/asimeow/
./asimeow init

# Create a default configuration file in the current directory
./asimeow init --local

# Create a default configuration file at a specific path
./asimeow init --path /path/to/config.yaml
```

Note: This tool requires macOS and uses the `tmutil` command to manage Time Machine exclusions. You may need to run it with sudo for some operations.

### Configuration File Location

Asimeow looks for configuration files in the following order:

1. Path specified with the `-c` or `--config` flag
2. `config.yaml` in the current directory
3. `~/.config/asimeow/config.yaml` in the user's home directory

If no configuration file is found, Asimeow will display an error message with instructions on how to create one.

## Configuration

The tool uses a YAML configuration file with the following structure:

```yaml
roots:
  - path: ~/works/projects/  # Paths to explore (~ is expanded to home directory)
  - path: /another/path/

rules:
  - name: "net"              # Rule name
    file_match: "*.csproj"   # Glob pattern to match files
    exclusions:              # Directories to exclude from Time Machine when the pattern is matched
      - "obj"
      - "bin"
      - "packages"
  - name: "rust"
    file_match: "cargo.toml"
    exclusions:
      - "target"
  - name: "node"
    file_match: "package.json"
    exclusions:
      - "node_modules"
      - "dist"
      - "build"
  - name: "python"
    file_match: "requirements.txt"
    exclusions:
      - "venv"
      - "__pycache__"
      - ".pytest_cache"
  - name: "markdown"         # Rule with no exclusions
    file_match: "*.md"
    exclusions: []           # Empty exclusions list
```

### Example Configuration

Here's a minimal example of a configuration file:

```yaml
# Define the root directories to scan
roots:
  - path: ~/projects/  # Will be expanded to your home directory
  - path: ~/work/      # You can specify multiple roots

# Define rules for different project types
rules:
  # Node.js projects
  - name: "node"
    file_match: "package.json"
    exclusions:
      - "node_modules"
      - "dist"

  # Rust projects
  - name: "rust"
    file_match: "Cargo.toml"
    exclusions:
      - "target"
```

When you run `asimeow init`, a default configuration file will be created with common rules for various project types. You can then customize it to suit your needs.

### Configuration Options

- **roots**: List of base paths to process
  - **path**: Directory path to start exploring (supports ~ for home directory)

- **rules**: List of rules to apply
  - **name**: Descriptive name for the rule
  - **file_match**: Glob pattern to match files or directories
  - **exclusions**: List of directory names to exclude from Time Machine backups (can be empty)

## How It Works

1. The tool reads the configuration file to get root paths and rules
2. For each root path, it recursively explores all subdirectories using multiple worker threads
3. When a file matching a rule's pattern is found (e.g., package.json, cargo.toml), it checks for the existence of excluded directories
4. For each excluded directory that exists (e.g., node_modules, target), it:
   - Checks if the directory is already excluded from Time Machine
   - If not, adds it to Time Machine exclusions using `tmutil addexclusion`
   - Displays the status with visual indicators (✅ for newly excluded, 🟡 for already excluded)
5. Directories listed in the exclusions are not explored further
6. With the verbose flag (-v), additional information is displayed

## Example Output

### Default Output (Concise)

```
✅ /Users/user/works/projects/my-rust-project/target - rust
🟡 /Users/user/works/projects/my-node-project/node_modules - node
✅ /Users/user/works/projects/my-node-project/dist - node

Total paths processed: 42
Total exclusions found: 3
Newly excluded from Time Machine: 2
```

The output uses:
- ✅ Green check mark: Directory newly excluded from Time Machine
- 🟡 Yellow circle: Directory already excluded from Time Machine

### Verbose Output (-v flag)

```
Asimeow - Folder Analysis Tool
-----------------------------
Reading config from: config.yaml
Using 4 worker threads

Loaded 3 rules:
  - rust (pattern: cargo.toml, exclusions: target)
  - node (pattern: package.json, exclusions: node_modules, dist)
  - markdown (pattern: *.md, exclusions: )

Processing path: /Users/user/works/projects
Found match for rule 'rust' at: /Users/user/works/projects/my-rust-project/cargo.toml
✅ /Users/user/works/projects/my-rust-project/target - rust
  → Excluded from Time Machine: /Users/user/works/projects/my-rust-project/target
Found match for rule 'node' at: /Users/user/works/projects/my-node-project/package.json
🟡 /Users/user/works/projects/my-node-project/node_modules - node
  → Already excluded from Time Machine
✅ /Users/user/works/projects/my-node-project/dist - node
  → Excluded from Time Machine: /Users/user/works/projects/my-node-project/dist
Found match for rule 'markdown' at: /Users/user/works/projects/README.md
  No exclusions defined for this rule

Total paths processed: 42
Total exclusions found: 3
Newly excluded from Time Machine: 2
```

## Why Use Asimeow?

Developers often have large directories of build artifacts, dependencies, and generated files that:
1. Take up significant space in Time Machine backups
2. Are easily regenerated and don't need to be backed up
3. Can slow down backup and restore operations

Asimeow automatically identifies and excludes these directories based on project types, saving backup space and improving Time Machine performance.

## Contributing

Contributions are welcome! Here's how you can contribute to Asimeow:

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-new-feature`
3. Make your changes and commit them: `git commit -am 'Add some feature'`
4. Push to the branch: `git push origin feature/my-new-feature`
5. Submit a pull request

### Continuous Integration

This project uses GitHub Actions for continuous integration and deployment:

- All pull requests and pushes to the main branch are automatically tested
- Tests run on macOS to ensure compatibility with the target platform
- Code formatting and linting are checked using `cargo fmt` and `clippy`
- When a new version tag (v*) is pushed, the package is automatically published to crates.io

## Acknowledgments

Many thanks and kudos to the inspiring project [Asimov](https://github.com/stevegrunwell/asimov) by Steve Grunwell, which provided the original concept for this tool. Asimeow extends the idea with multi-threading, rule-based detection, and a more developer-focused approach.