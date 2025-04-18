# Asimeow

[![CI/CD](https://github.com/mdnmdn/asimeow/actions/workflows/pipeline.yml/badge.svg)](https://github.com/mdnmdn/asimeow/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/asimeow.svg)](https://crates.io/crates/asimeow)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A command-line tool that automatically manages macOS Time Machine exclusions for developer projects.
It recursively analyzes folders according to rules defined in a configuration file and excludes development
artifacts from Time Machine backups.

## Features

- Recursively explores directories from specified root paths
- Identifies files matching patterns defined in rules (like package.json, cargo.toml, etc.)
- Automatically excludes development artifacts from Time Machine backups
- Provides commands to manually exclude or include specific files and directories
- Allows listing and checking the exclusion status of files and directories
- Multi-threaded for fast processing of large directory structures

## Acknowledgments

Many thanks and kudos to Steve Grunwell and it's inspiring [asimov](https://github.com/stevegrunwell/asimov) project,
which provided the original concept for this tool. 
Asimeow aims to be a spiritual successor to the original asimov project, building upon its core concept with several
improvements. These enhancements include more adaptable rule configurations, multi-threaded tree traversal, 
and more efficient processing capabilities.


## Installation

### Using Homebrew

```bash
brew tap mdnmdn/asimeow
brew install asimeow
```

To run asimeow as a scheduled service (run every 6 hours):

```bash
brew services start asimeow
```

### From GitHub Releases

1. Go to the [Releases page](https://github.com/mdnmdn/asimeow/releases)
2. Download the appropriate binary for your Mac:
   - Intel Mac: `asimeow-x86_64-apple-darwin.zip`
   - Apple Silicon Mac: `asimeow-aarch64-apple-darwin.zip`
3. Extract the zip file
4. Move the binary to a location in your PATH:

```bash
# Example
unzip asimeow-x86_64-apple-darwin.zip
chmod +x asimeow
sudo mv asimeow /usr/local/bin/asimeow
```

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

# List exclusions in the current directory
./asimeow list

# List exclusions in a specific directory (with trailing slash)
./asimeow list /path/to/directory/

# Check exclusion status of a specific file or directory (without trailing slash)
./asimeow list /path/to/file

# Explicitly exclude a specific file or directory from Time Machine backups
./asimeow exclude /path/to/file_or_directory

# Explicitly include a specific file or directory in Time Machine backups (remove exclusion)
./asimeow include /path/to/file_or_directory
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
```

### Example Configuration

Here's a minimal example of a configuration file:

```yaml
# Define the root directories to scan
roots:
  - path: ~/projects/  # Will be expanded to your home directory
  - path: ~/work/      # You can specify multiple roots

# Define directories to ignore during exploration
ignore:
  - .git              # Common directories to skip

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

- **ignore**: List of directory patterns to skip during exploration (e.g., `.git`, `node_modules`)
  - These directories will be completely ignored during the exploration process
  - Useful for improving performance by skipping large directories that don't need to be scanned

- **rules**: List of rules to apply
  - **name**: Descriptive name for the rule
  - **file_match**: Glob pattern to match files or directories
  - **exclusions**: List of directory names to exclude from Time Machine backups (can be empty)

## How It Works

### Automatic Exclusion Mode

1. The tool reads the configuration file to get root paths, ignore patterns, and rules
2. For each root path, it recursively explores all subdirectories using multiple worker threads
3. Directories matching the ignore patterns (e.g., `.git`) are skipped entirely during exploration
4. When a file matching a rule's pattern is found (e.g., package.json, cargo.toml), it checks for the existence of excluded directories
5. For each excluded directory that exists (e.g., node_modules, target), it:
   - Checks if the directory is already excluded from Time Machine
   - If not, adds it to Time Machine exclusions using `tmutil addexclusion`
   - Displays the status with visual indicators (✅ for newly excluded, 🟡 for already excluded)
6. Directories listed in the exclusions are not explored further
7. With the verbose flag (-v), additional information is displayed

### Manual Exclusion Commands

The tool also provides direct commands to manage Time Machine exclusions:

- `exclude <path>`: Explicitly excludes a specific file or directory from Time Machine backups
  - Checks if the path exists
  - Verifies if it's already excluded
  - Adds it to Time Machine exclusions if needed
  - Displays the result (✅ for newly excluded, 🟡 for already excluded)

- `include <path>`: Explicitly includes a specific file or directory in Time Machine backups
  - Checks if the path exists
  - Verifies if it's already included
  - Removes it from Time Machine exclusions if needed
  - Displays the result (✅ for newly included, or a message if already included)

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

### List Command Output

#### Listing a Directory

```
Listing contents of: /Users/user/projects/
------------------------------------
🟡 node_modules/
   package.json
   README.md
🟡 target/
   Cargo.toml
   src/

Legend:
🟡 - Excluded from Time Machine
  - Included in Time Machine
/ - Directory
```

#### Checking a Specific File or Directory

```
Status of directory: /Users/user/projects/target
------------------------------------
🟡 target/

Legend:
🟡 - Excluded from Time Machine
  - Included in Time Machine
/ - Directory
```

### Exclude Command Output

```
✅ Successfully excluded: /Users/user/projects/build
```

With verbose flag (-v):

```
Excluding directory from Time Machine: /Users/user/projects/build
✅ Successfully excluded: /Users/user/projects/build
```

If already excluded:

```
🟡 Already excluded: /Users/user/projects/node_modules
```

### Include Command Output

```
✅ Successfully included: /Users/user/projects/target
```

With verbose flag (-v):

```
Including directory in Time Machine: /Users/user/projects/target
✅ Successfully included: /Users/user/projects/target
```

If already included:

```
  Already included: /Users/user/projects/src
```

## Why Use Asimeow?

Developers often have large directories of build artifacts, dependencies, and generated files that:
1. Take up significant space in Time Machine backups
2. Are easily regenerated and don't need to be backed up
3. Can slow down backup and restore operations

Asimeow automatically identifies and excludes these directories based on project types, saving backup space and improving Time Machine performance.

## Roadmap

- [x] Analyze current time machine exclusions of specific paths
- [ ] Analyze exclusion folder "decay" in order to identify old and unused exclusions and clean from disk
- [ ] Provide detailed statistics about excluded directories and their sizes
- [ ] Improve tests
- [ ] Simplify access to configuration via cli options

## Contributing

Contributions are welcome! [Here's how you can contribute to Asimeow](CONTRIBUTING.md):

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

