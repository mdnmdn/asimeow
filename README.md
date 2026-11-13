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

# Show what would be excluded without changing anything
./asimeow --dry-run

# Full-screen view: progress, space saved and the excluded folders by size
./asimeow --visual

# Show how much space the exclusions save, from the last scan (no new scan)
./asimeow stats

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
   - Checks if the directory already carries Time Machine's exclusion mark
   - If not, adds the mark (the same one `tmutil addexclusion` writes, but set in-process, which is much faster)
   - Displays the status with visual indicators (✅ for newly excluded, 🟡 for already excluded)
6. Directories listed in the exclusions are not explored further, and symlinked directories are never followed
7. After the walk, every excluded folder is measured (the logical size of its files, hard links counted once).
   Sizes are cached in `~/Library/Caches/asimeow/state.json` and reused while the folder is unchanged
   (same inode and modification time) and the size is less than 7 days old, so repeat runs stay fast
8. With the verbose flag (-v), additional information is displayed

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

Asimeow prints only what changed: newly excluded folders, failures and warnings.
Folders that were already excluded are counted but not listed (use `-v` to list them).

### In a terminal

A live status line shows progress while the scan runs. It is replaced by a short summary at the end:

```
Using configuration: /Users/user/.config/asimeow/config.yaml
✅ ~/works/projects/my-rust-project/target  rust
✅ ~/works/projects/my-node-project/dist  node
⠹ 12,204 folders · ~/works/projects/website/src/components
```

```
✓ Done in 0.4s · 21,967 folders · 77 exclusions, 2 new (1 recreated)
  2.9 GB excluded · +668 MB this run · +0.6 GB since last run
  2 folders could not be read; run with -v to list them
```

"Recreated" means the folder was excluded on an earlier run but lost its mark, which
happens when it is deleted and created again (`rm -rf node_modules && npm install`).

Colors follow your terminal theme; set `NO_COLOR=1` to turn them off.

### Visual mode (`--visual`)

A full-screen view instead of lines: progress (with an estimate from the last run), the
space saved, the excluded folders grouped by area and sorted by size, and recent activity.

```
┌ asimeow 0.2.1 ───────────────────────────────────────────────────────── done in 0.3s ┐
│████████████████████████████████████████100% █████████████████████████████████████████│
│23,846 folders · 77 exclusions, 0 new                                                 │
│✓ Scan complete. Browse the folders below, q to quit.                                 │
└──────────────────────────────────────────────────────────────────────────────────────┘
┌ Savings ─────────────────────────────────────────────────────────────────────────────┐
│Excluded     3.0 GB    in 77 folders       New this run +0 B      0 new · 0 recreated │
│Last run     3.0 GB                        Change       +0 B since last run           │
└──────────────────────────────────────────────────────────────────────────────────────┘
┌ Where the space goes ──────────────────────────────────────────────── sorted by size ┐
│▾ ~/projects                                                   644 MB ████████████████│
│    · api/target                                 rust                602 MB           │
│    · notes/.venv                                python-pyproject   42.0 MB           │
│▾ ~/work                                                       527 MB █████████████   │
│    · web/node_modules                           node                514 MB           │
└──────────────────────────────────────────────────────────────────────────────────────┘
┌ Activity ────────────────────────────────────────────────────────────────────────────┐
│Cache: 77 known exclusions, 3.0 GB at the last run                                    │
│✓ Done: 0 would be excluded (dry run, nothing written)                                │
└──────────────────────────────────────────────────────────────────────────────────────┘
 q quit · ↑↓ move · ←→ fold · n only changes · dry run: nothing is written
```

Keys: `↑`/`↓` (or `j`/`k`) move, `←`/`→` fold and unfold an area, `Enter` toggles,
`n` shows only folders that changed in this run, `q` or `Esc` quits. The view stays open
after the scan; quitting during a scan stops it. On quit, the changes and the summary are
printed to the normal screen. Colors follow your terminal theme.

### In scripts and log files

When the output is not a terminal (a pipe, a file, a launchd or `brew services` log),
asimeow prints plain lines with absolute paths and ends with a `key=value` summary line:

```
Using configuration: /Users/user/.config/asimeow/config.yaml
✅ /Users/user/works/projects/my-rust-project/target - rust
✅ /Users/user/works/projects/my-node-project/dist - node
summary: folders=21967 exclusions=77 new=2 recreated=1 failed=0 unreadable=2 excluded_bytes=2903197910 new_bytes=668000000 seconds=0.42
```

Failures (`❌`) and warnings (`⚠️`) go to stderr. The exit code is 1 if any folder could not be excluded.

The output uses:
- ✅ Folder newly excluded from Time Machine (or that would be, with `--dry-run`)
- 🟡 Folder already excluded (listed with `-v`)
- ❌ Folder that could not be excluded

### Verbose Output (-v flag)

`-v` adds the loaded rules, every processed folder, every rule match, already-excluded
folders and folders that could not be read:

```
Asimeow - Time Machine Exclusion Tool
------------------------------------
Using 4 worker threads
Reading config from: /Users/user/.config/asimeow/config.yaml

Loaded 2 rules:
  - rust (pattern: cargo.toml, exclusions: target)
  - node (pattern: package.json, exclusions: node_modules, dist)

Using configuration: /Users/user/.config/asimeow/config.yaml
Processing path: /Users/user/works/projects
Found match for rule 'rust' at: /Users/user/works/projects/my-rust-project/cargo.toml
✅ /Users/user/works/projects/my-rust-project/target - rust
Found match for rule 'node' at: /Users/user/works/projects/my-node-project/package.json
🟡 /Users/user/works/projects/my-node-project/node_modules - node
✅ /Users/user/works/projects/my-node-project/dist - node
Size of /Users/user/works/projects/my-rust-project/target: 548 MB (measured)
Size of /Users/user/works/projects/my-node-project/node_modules: 212 MB (cached)
Size of /Users/user/works/projects/my-node-project/dist: 1.2 MB (measured)
summary: folders=42 exclusions=3 new=2 recreated=0 failed=0 unreadable=0 excluded_bytes=761200000 new_bytes=549200000 seconds=0.05
```

### Stats Command Output

```
2.9 GB excluded in 77 folders · last scan 2 h ago (21,967 folders scanned)

Largest:
   548 MB  ~/projects/api/target  rust
   514 MB  ~/projects/web/node_modules  node
   388 MB  ~/tools/editor-plugins/node_modules  node
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
- [x] Provide detailed statistics about excluded directories and their sizes
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

