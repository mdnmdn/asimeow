# Setting Up Your Homebrew Tap

This guide explains how to set up a Homebrew tap repository for asimeow.

## What is a Homebrew Tap?

A Homebrew tap is a repository of formulae (package definitions) for Homebrew, the macOS package manager. By creating a tap, you allow users to install your software using Homebrew.

## Steps to Create Your Tap Repository

1. Create a new GitHub repository named `homebrew-asimeow`
   - The repository name must start with `homebrew-`
   - This will be accessible as `mdnmdn/asimeow` in Homebrew

2. Initialize the repository with a README.md file

3. Create a directory structure for your formulae:
   ```
   homebrew-asimeow/
   ├── Formula/
   │   └── asimeow.rb
   └── README.md
   ```

4. Copy the formula file from this repository:
   - Copy `Formula/asimeow.rb` to your new repository's `Formula/` directory
   - The GitHub workflow will automatically update the SHA256 hashes when a new release is created

5. Update the README.md with installation instructions:
   ```markdown
   # Homebrew Tap for Asimeow

   This repository contains Homebrew formulae for [Asimeow](https://github.com/mdnmdn/asimeow).

   ## How to Install

   ```bash
   # Add the tap
   brew tap mdnmdn/asimeow

   # Install asimeow
   brew install asimeow
   ```

   To run asimeow as a scheduled service:

   ```bash
   brew services start asimeow
   ```
   ```

## Updating the Formula

When you release a new version of asimeow:

1. Create a new release with a tag (e.g., v0.1.2) in the main repository
2. The GitHub workflow will automatically:
   - Build the binaries for Intel and Apple Silicon Macs
   - Create a GitHub release with the binaries
   - Calculate SHA256 hashes for the binaries and source tarball
   - Update the formula in your tap repository with the new version and hashes
3. Users can then install the new version using `brew upgrade asimeow`

## Testing Your Tap Locally

Before publishing, you can test your tap locally:

1. Clone your tap repository:
   ```bash
   git clone https://github.com/mdnmdn/homebrew-asimeow.git
   ```

2. Install from the local tap:
   ```bash
   brew install --build-from-source ./homebrew-asimeow/Formula/asimeow.rb
   ```

## Resources

- [Homebrew Documentation on Taps](https://docs.brew.sh/Taps)
- [Creating and Maintaining a Tap](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)