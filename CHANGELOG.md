# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Homebrew installation support
- Automated Homebrew formula updates via GitHub Actions


## [0.1.3] - 2023-11-16

### Fixed
- publish to cargo and github

## [0.1.2] - 2023-11-16

### Added
- Automated GitHub releases with pre-built binaries for macOS (Intel and Apple Silicon)
- Cross-platform binary distribution via GitHub releases

### Fixed
- publish to cargo registry

## [0.1.1] - 2023-11-16

### Added
- GitHub Actions CI/CD pipeline for automated testing and deployment
- Automated publishing to crates.io on new version tags
- Improved documentation with badges and contribution guidelines
- Support for the ignore setting to enhance directory exclusion capabilities.

### Fixed
- Bug in the exploration of excluded folders, ensuring directories specified in the ignore settings are properly excluded.

## [0.1.0] - 2023-11-15

### Added
- Initial release
- Support for recursive directory exploration
- Rule-based detection of project files
- Automatic Time Machine exclusion management
- Multi-threaded processing
- Configuration via YAML files