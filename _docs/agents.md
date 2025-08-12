## Asimeow agents: goal, behaviours, and how it works

This document explains the intent of Asimeow, the key behaviours exposed via its CLI, and the internal "agents" (components) that collaborate to perform automatic Time Machine exclusions on macOS.

### Goal

- **Primary goal**: Identify development artifacts (e.g., `node_modules`, `target`, `dist`) inside project folders and exclude them from macOS Time Machine backups to save space and speed up backups.
- **Approach**: Recursively traverse configured root folders, match project indicators (like `package.json`, `Cargo.toml`), and exclude rule-defined directories using `tmutil`.

## Behaviours (what the tool does)

- **Automatic scan and exclude**: Walks directories under configured `roots`, matches `rules` via `file_match` glob patterns, and excludes each rule's `exclusions` directories when found.
- **Ignore directories**: Skips directories matching `ignore` patterns (glob on the directory name, e.g., `.git`).
- **Manual control**:
  - `list [path]`: Show Time Machine exclusion status for a directory or a single path.
  - `exclude <path>`: Force-exclude a directory or file via `tmutil addexclusion`.
  - `include <path>`: Remove exclusion via `tmutil removeexclusion`.
- **Initialization**: `init` generates a default `config.yaml` either locally or at `~/.config/asimeow/config.yaml` with common presets for many ecosystems.
- **Reporting**: Prints concise status lines (✅ newly excluded, 🟡 already excluded) and summary totals (processed paths, exclusions found, newly excluded) when relevant or in verbose mode.

## How it works (internal architecture)

### CLI and entrypoint

- File: `src/main.rs`
- **Argument parsing**: Uses `clap` for flags and subcommands:
  - `-c, --config` path (defaults to auto-detection if left as the default value `config.yaml`)
  - `-v, --verbose`
  - `-t, --threads <N>` worker threads for traversal
  - Subcommands: `init`, `version`, `list`, `exclude`, `include`
- **Execution flow**:
  1. If a subcommand is provided, execute it immediately and exit.
  2. Otherwise, load configuration (auto-detect path) and run the explorer with the selected thread count.

### Configuration agent

- File: `src/config.rs`
- **Schema**:
  - `Config { roots: Vec<Root>, ignore: Vec<String>, rules: Vec<Rule> }`
  - `Root { path: String }`
  - `Rule { name: String, file_match: String, exclusions: Vec<String> }`
- **Responsibilities**:
  - `create_default_config(local, path)`: creates a YAML config with common rules (Node, Rust, Python, etc.). Ensures directories and writes file.
  - `find_config_file(specified)`: resolves config from explicit path, `./config.yaml`, or `~/.config/asimeow/config.yaml`.
  - `load_config(path, verbose)`: reads, parses YAML, prints loaded rules in verbose mode, and validates `roots`.
  - `expand_tilde(path)`: resolves `~/` to the user home directory.

### Explorer agent (orchestrator)

- File: `src/explorer.rs`
- **State model**:
  - `State` with thread-safe counters and a shared queue: `folder_queue`, `exclusion_found`, `processed_paths`, `active_tasks`, `processing_complete`, `newly_excluded` (all protected by `RwLock`).
  - Simple in-memory work queue (`Vec<PathBuf>`) managed under locks; workers pop from the front.
- **Workers**:
  - `run_workers(state, rules, thread_count, verbose, ignore_patterns)`: spawns `thread_count` threads. Each thread repeatedly pulls a path from the queue and calls `process_path()` until the queue empties and no tasks are active.
  - Completion is detected when the queue is empty and `active_tasks == 0`, then `processing_complete` is set.
- **Traversal**:
  - `run_explorer(config, threads, verbose)`: enqueues each `root` (with `~` expansion), then starts workers. After completion, prints totals.
  - `process_path(path, state, rules, verbose, ignore_patterns)`:
    - Validates path exists and is a directory.
    - Applies ignore checks against the current directory name using glob patterns from `ignore`.
    - Reads entries once, collecting subdirectories while evaluating rule matches.
    - For each file/entry in the current directory, matches `rule.file_match` using glob semantics (case-insensitive via lowercase comparisons).
    - On a rule match:
      - For each `exclusions` item, builds `exclusion_path = current_dir / exclusion` and attempts exclusion.
      - Records `directory_to_ignore` to avoid descending into newly excluded directories.
      - Special-case: if `exclusions` contains "." or "..", the function returns early (treat as do-not-descend behaviour for current or parent).
    - Finally, enqueues subdirectories that are not ignored and not among the excluded dir names.

### Time Machine integration agent

- File: `src/explorer.rs`
- **Functions**:
  - `is_excluded_from_timemachine(path)`: runs `tmutil isexcluded <path>` and parses `[Excluded]`.
  - `exclude_from_timemachine(path)`: runs `tmutil addexclusion <path>`; returns false if already excluded.
  - `include_in_timemachine(path)`: runs `tmutil removeexclusion <path>`; returns false if already included.
- **User-facing commands**:
  - `exclude_path(path, verbose)` / `include_path(path, verbose)` wrap the above with path expansion, type detection (file/dir), and user messages.
  - `list_exclusions(path?)`: directory listing mode (if a directory ending with `/` or no path provided) or single item status mode; prints legend and markers.

## Data flow summary

1. CLI parses args and subcommands.
2. If no subcommand: configuration is located and parsed.
3. Roots are enqueued; worker threads process the queue concurrently.
4. Each directory read checks for rule indicators and applies exclusions.
5. Exclusions are attempted with `tmutil`, producing ✅ or 🟡 output lines.
6. Counters are updated and a concise summary is printed at the end.

## Configuration quick reference

- **roots**: list of starting directories. `~` is supported.
- **ignore**: directory-name patterns to skip entirely (e.g., `.git`, `node_modules`).
- **rules**: matches project indicators and lists directories to exclude when matched.

Example snippet:

```yaml
roots:
  - path: ~/projects/
ignore:
  - .git
rules:
  - name: node
    file_match: package.json
    exclusions: [node_modules, dist]
  - name: rust
    file_match: Cargo.toml
    exclusions: [target]
```

## Operational notes

- **Platform**: macOS only. Requires `tmutil` available in PATH.
- **Permissions**: Some exclusions may require elevated privileges; run the tool with sufficient rights if needed.
- **Performance**: Multi-threaded traversal with a shared queue; simple locking can become a bottleneck on very large trees, but is adequate for typical developer machines.
- **Pattern matching**: Rule file-matching and ignore matching use glob semantics on lowercased names of directory entries.

### CLI quick reference

- `./asimeow` — Run with automatic config detection
- `./asimeow -c path/to/config.yaml` — Use custom config
- `./asimeow -v` — Verbose output
- `./asimeow -t 8` — Set worker threads
- `./asimeow init` — Create default config in `~/.config/asimeow/`
- `./asimeow init --local` — Create `config.yaml` in current directory
- `./asimeow list [path]` — List Time Machine exclusions
- `./asimeow exclude <path>` — Exclude a path from backups
- `./asimeow include <path>` — Include a path in backups

### Dependencies

- `serde`, `serde_yaml` — Configuration (de)serialization
- `glob` — Glob-style pattern matching for rules
- `clap` — Command-line argument parsing
- `anyhow` — Error handling
- `dirs` — Home directory resolution for `~/`

### Developer notes

- Build: `cargo build` (or `cargo build --release`)
- Test: `cargo test` (tests live under `tests/`, entry in `tests/mod.rs`)
- Format and lint: `cargo fmt` and `cargo clippy`
- Run locally: `cargo run -- [args]` (e.g., `cargo run -- -v`)

### Justfile (convenience tasks)

Use `just` to run common workflows quickly:

- **format**: `just format` — runs `cargo fmt --all`
- **lint**: `just lint` — runs `cargo clippy -- -D warnings`
- **check**: `just check` — runs `cargo check`
- **build**: `just build` — runs `cargo build`

If needed, install `just` via Homebrew: `brew install just`.

### CI/CD

- Runs on macOS in GitHub Actions
- Requires passing `cargo fmt` and `cargo clippy`
- Tests run on PRs and pushes to `main`
- Releases triggered by version tags (`v*`), with publish to crates.io

## Extending or customizing

- Add or modify `rules` in your `config.yaml` to support new ecosystems.
- Add entries to `ignore` to skip heavy directories you never want scanned.
- Use `-t` to tune concurrency for your machine.
- Use `init` to bootstrap a sensible default configuration.

## Glossary of agents (conceptual)

- **CLI agent**: Parses user intent and routes to subcommands or the explorer.
- **Configuration agent**: Finds, reads, and writes `config.yaml` and expands paths.
- **Explorer agent**: Owns traversal, queueing, matching rules, and recording results.
- **Worker agents**: Concurrent threads that process directories from the shared queue.
- **Time Machine agent**: Thin shell-out layer that interacts with `tmutil` for inclusion/exclusion queries and commands.

# Instructions

## Directive

- simplicity is the ultimate perfetcion
- be pragmatic and brutally honest
- write simply and clean tests for each task, but without overgeneering
- at end of the task update the CHANGELOG
- when a task is complete `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` and make sure are ok
- write all the task realated temporary informations in the _docs/wip, for example markdown plans.
- if the task are small there is no 