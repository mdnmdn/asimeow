# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `--dry-run`: report what would be excluded without changing anything

### Changed
- Much faster scans. Exclusion marks are read from the folder's extended attribute and
  written with the CoreServices call that `tmutil` uses underneath, instead of spawning
  `tmutil` (about 60 ms per check and several seconds per write). The mark is identical
  to the one `tmutil addexclusion` writes.
- Rules and ignore patterns are compiled once per run instead of once per directory entry
- `exclude` and `include` report failures instead of "already excluded/included";
  `include` says when a path stays excluded through a parent folder or a system rule
- A scan no longer treats folders inside system-excluded locations (such as `/tmp`) or
  inside an excluded parent as already excluded: they get their own mark

### Fixed
- A race in the worker queue could end a scan early and silently skip folders
- Symlinked directories are no longer followed (no more loops or walks into other volumes)
- A failed exclusion was reported as "already excluded"; it's now reported as an error

## [0.2.1]

### Fixed
- Avoid traversing already excluded directories

### Added
- Justfile with minimal tasks: `format`, `lint`, `check`, `build`
- Added agents context and flows

## [0.1.9] - 2025-03-24

### Added
- Homebrew installation support
- Automated Homebrew formula updates via GitHub Actions

## [0.1.3] - 2024-11-16

### Fixed
- publish to cargo and github

## [0.1.2] - 2024-11-16

### Added
- Automated GitHub releases with pre-built binaries for macOS (Intel and Apple Silicon)
- Cross-platform binary distribution via GitHub releases

### Fixed
- publish to cargo registry

## [0.1.1] - 2024-11-16

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