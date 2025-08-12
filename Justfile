# Minimal task runner for common workflows

# Format code
format:
	cargo fmt --all

# Lint with clippy (treat warnings as errors)
lint:
	cargo clippy -- -D warnings

# Type-check without building binaries
check:
	cargo check

# Build the project (debug)
build:
	cargo build

full-lint:
	@just format
	@just lint
