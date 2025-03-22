# Contributing to Asimeow

Thank you for your interest in contributing to Asimeow! This document provides guidelines and instructions for contributing to the project.

## Development Workflow

1. Fork the repository on GitHub
2. Clone your fork to your local machine
3. Create a new branch for your feature or bugfix
4. Make your changes
5. Run tests locally to ensure everything works
6. Commit your changes with clear, descriptive commit messages
7. Push your branch to your fork on GitHub
8. Open a pull request against the main repository

## Running Tests Locally

Before submitting a pull request, make sure all tests pass:

```bash
cargo test
```

Also check code formatting and run the linter:

```bash
cargo fmt --all -- --check
cargo clippy -- -D warnings
```

## Continuous Integration

This project uses GitHub Actions for CI/CD:

- All pull requests and pushes to the main branch trigger the CI pipeline
- The pipeline runs on macOS to ensure compatibility with the target platform
- Tests, formatting checks, and linting are performed automatically

## Release Process

Releases are automated through GitHub Actions:

1. Update the version in `Cargo.toml`
2. Update the `CHANGELOG.md` with details of the changes
3. Commit these changes and push to the main branch
4. Create and push a new tag with the version number:
   ```bash
   git tag v0.1.1
   git push origin v0.1.1
   ```
5. The GitHub Actions workflow will automatically:
   - Run all tests
   - If tests pass, publish the new version to crates.io

## Setting up for Publishing

To enable automatic publishing to crates.io, a maintainer needs to:

1. Log in to crates.io and generate an API token
2. Add the token as a GitHub repository secret named `CRATES_IO_TOKEN`

## Code Style

- Follow the Rust standard formatting (enforceable with `cargo fmt`)
- Use meaningful variable and function names
- Add comments for complex logic
- Write tests for new functionality

## Commit Messages

- Use clear, descriptive commit messages
- Start with a short summary line (50 chars or less)
- Optionally followed by a blank line and a more detailed explanation

## Pull Request Process

1. Ensure your code passes all tests and lint checks
2. Update documentation if necessary
3. Add relevant tests for new functionality
4. Update the CHANGELOG.md with details of changes
5. The PR will be reviewed by maintainers who may request changes
6. Once approved, a maintainer will merge your PR

Thank you for contributing to Asimeow!