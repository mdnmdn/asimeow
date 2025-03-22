# Asimaw

A command-line tool that recursively analyzes folders according to rules defined in a configuration file.

## Features

- Recursively explores directories from specified root paths
- Identifies files matching patterns defined in rules
- Prints exclusion lists for each matched rule
- Skips exploring excluded directories

## Installation

```bash
cargo build --release
```

The executable will be available at `target/release/asimaw`.

## Usage

```bash
# Use default config.yaml in current directory
./asimaw

# Specify a custom config file
./asimaw -c /path/to/config.yaml

# Enable verbose output
./asimaw -v
```

## Configuration

The tool uses a YAML configuration file with the following structure:

```yaml
roots:
  - path: ~/works/projects/  # Paths to explore (~ is expanded to home directory)
  - path: /another/path/

rules:
  - name: "net"              # Rule name
    file_match: "*.csproj"   # Glob pattern to match files
    exclusions:              # Directories to exclude when the pattern is matched
      - "obj"
      - "bin"
      - "packages"
  - name: "rust"
    file_match: "cargo.toml"
    exclusions:
      - "target"
  - name: "markdown"         # Rule with no exclusions
    file_match: "*.md"
    exclusions: []           # Empty exclusions list
```

### Configuration Options

- **roots**: List of base paths to process
  - **path**: Directory path to start exploring (supports ~ for home directory)

- **rules**: List of rules to apply
  - **name**: Descriptive name for the rule
  - **file_match**: Glob pattern to match files or directories
  - **exclusions**: List of directory names to exclude from exploration (can be empty)

## How It Works

1. The tool reads the configuration file to get root paths and rules
2. For each root path, it recursively explores all subdirectories
3. When a file matching a rule's pattern is found, it checks for the existence of excluded directories
4. For each excluded directory that exists, it prints the path and rule name
5. Directories listed in the exclusions are not explored further
6. With the verbose flag (-v), additional information is displayed

## Example Output

### Default Output (Concise)

```
/home/user/works/projects/my-rust-project/target - rust
/home/user/works/projects/my-node-project/node_modules - node
/home/user/works/projects/my-node-project/dist - node
```

### Verbose Output (-v flag)

```
Asimaw - Folder Analysis Tool
-----------------------------
Reading config from: config.yaml

Loaded 3 rules:
  - rust (pattern: cargo.toml, exclusions: target)
  - node (pattern: package.json, exclusions: node_modules, dist)
  - markdown (pattern: *.md, exclusions: )

Processing: /home/user/works/projects
Found match for rule 'rust' at: /home/user/works/projects/my-rust-project/cargo.toml
/home/user/works/projects/my-rust-project/target - rust
Found match for rule 'node' at: /home/user/works/projects/my-node-project/package.json
/home/user/works/projects/my-node-project/node_modules - node
/home/user/works/projects/my-node-project/dist - node
Found match for rule 'markdown' at: /home/user/works/projects/README.md
  No exclusions defined for this rule
```