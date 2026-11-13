## Asimeow agents: goal, behaviours, and how it works

This document explains the intent of Asimeow, the key behaviours exposed via its CLI, and the internal "agents" (components) that collaborate to perform automatic Time Machine exclusions on macOS.

### Goal

- **Primary goal**: Identify development artifacts (e.g., `node_modules`, `target`, `dist`) inside project folders and exclude them from macOS Time Machine backups to save space and speed up backups.
- **Approach**: Recursively traverse configured root folders, match project indicators (like `package.json`, `Cargo.toml`), and exclude rule-defined directories by setting Time Machine's sticky exclusion mark (the one `tmutil addexclusion` writes).

## Behaviours (what the tool does)

- **Automatic scan and exclude**: Walks directories under configured `roots`, matches `rules` via `file_match` glob patterns, and excludes each rule's `exclusions` directories when found.
- **Ignore directories**: Skips directories matching `ignore` patterns (glob on the directory name, e.g., `.git`).
- **Manual control**:
  - `list [path]`: Show Time Machine exclusion status for a directory or a single path.
  - `exclude <path>`: Force-exclude a directory or file by setting the sticky exclusion mark.
  - `include <path>`: Remove the sticky exclusion mark (and report if a parent folder or a system rule still excludes it).
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
  - `State`: the `verbose` and `dry_run` settings plus atomic counters (`processed_paths`, `exclusions_found`, `newly_excluded`).
  - A private `Queue`: a `VecDeque<PathBuf>` and a `pending` count (directories queued or being processed) under one `Mutex`, with a `Condvar` for idle workers.
- **Workers**:
  - `run_workers(roots, state, matcher, thread_count)`: spawns scoped threads. Each one pops a directory, calls `process_path()`, and hands the returned subdirectories back with `Queue::finish`, which adds the children and retires the parent in one locked step. The walk ends when `pending` reaches 0, so it can't end while a worker still holds work.
- **Traversal**:
  - `run_explorer(config, threads, verbose, dry_run)`: expands the roots (`~`), compiles the rules once into a `Matcher`, runs the workers, and prints totals.
  - `process_path(path, state, matcher)`:
    - Skips directories whose name matches an `ignore` pattern.
    - Reads entries once. It matches each name against the rules and collects subdirectories with `DirEntry::file_type()`, which does not follow symlinks.
    - On a rule match, `process_exclusion` checks each `exclusions` item (`current_dir / exclusion`) that exists: already marked → 🟡; otherwise it writes the mark (or only reports it with `--dry-run`) → ✅; a failed write → ❌ on stderr.
    - Never descends into excluded folders. If `exclusions` contains "." or "..", it stops below this directory.
    - Returns the remaining subdirectories to walk next.

### Rules agent

- File: `src/rules.rs`
- `Matcher::new(rules, ignore)` compiles everything once. Literal `file_match` names (most rules) go into a hash map; real globs (`*.csproj`) are matched in order. Matching is case-insensitive and the first rule in config order wins.
- `is_ignored(dir_name)`: glob match of `ignore` patterns on a directory name (case-sensitive).

### Time Machine integration agent

- File: `src/tm.rs`
- **Functions**:
  - `has_exclusion_mark(path)`: reads the sticky exclusion xattr `com.apple.metadata:com_apple_backup_excludeItem` (no subprocess). Ignores system-wide and inherited exclusions.
  - `set_exclusion_mark(path, exclude)`: sets or clears the mark with CoreServices `CSBackupSetItemExcluded`, the call `tmutil addexclusion` and cargo use underneath (byte-identical mark, under 1 ms instead of seconds), then verifies it.
  - `is_excluded(path)`: runs `tmutil isexcluded <path>` (includes system and inherited exclusions). Used by `list`, `exclude` and `include`.
- **User-facing commands** (in `src/explorer.rs`):
  - `exclude_path(path, verbose)` / `include_path(path, verbose)` wrap the above with path expansion, type detection (file/dir), and user messages.
  - `list_exclusions(path?)`: directory listing mode (if a directory ending with `/` or no path provided) or single item status mode; prints legend and markers.

## Data flow summary

1. CLI parses args and subcommands.
2. If no subcommand: configuration is located and parsed.
3. Roots are enqueued; worker threads process the queue concurrently.
4. Each directory read checks for rule indicators and applies exclusions.
5. Exclusion marks are read and written in-process, producing ✅ or 🟡 output lines.
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
- **Performance**: Multi-threaded traversal with one shared queue. No subprocess is spawned during a scan; `tmutil` is used only by `list`, `exclude` and `include`.
- **Pattern matching**: Rule file-matching and ignore matching use glob semantics on lowercased names of directory entries.

### CLI quick reference

- `./asimeow` — Run with automatic config detection
- `./asimeow -c path/to/config.yaml` — Use custom config
- `./asimeow -v` — Verbose output
- `./asimeow -t 8` — Set worker threads
- `./asimeow --dry-run` — Report what would be excluded without changing anything
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
- No other runtime dependencies: the Time Machine mark is read with `getxattr` and written with CoreServices via small FFI declarations in `src/tm.rs`

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
- **Rules agent**: Compiles rules and ignore patterns once and matches entry names.
- **Time Machine agent**: Reads and writes the exclusion mark in-process; shells out to `tmutil isexcluded` for status queries.

# Instructions

## Directives

- simplicity is the ultimate perfetcion
- be pragmatic and brutally honest
- write simply and clean tests for each task, but without overgeneering
- at end of the task update the CHANGELOG
- when a task is complete `cargo fmt --all -- --check` and `cargo clippy -- -D warnings` and make sure are ok
- write all the task realated temporary informations in the _docs/wip, for example markdown plans.
- if the task are small there is no need for plan
- if plan is asked by the user don't start to implement unless requested