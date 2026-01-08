# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Global `--yes` / `-y` flag**: Skip all interactive prompts automatically. Useful for scripting and AI agent integration.
- **Global `--json` flag**: Output in JSON format for machine parsing (infrastructure added, per-command support coming).
- **Global `--verbose` / `-v` flag**: Enable verbose debug logging.
- **`--dry-run` flag**: Preview destructive operations without executing them. Added to:
  - `repo delete`
  - `release delete`
  - `release asset delete`
  - `actions variables delete`
  - `actions secrets delete`
  - `org team delete`
  - `org team repo rm`
  - `org team member rm`
  - `user key delete`
  - `user gpg delete`
- **`--force` / `-f` flag**: Skip confirmation prompts on destructive operations. Added to commands that previously used interactive confirmation or had no safeguard.
- **AGENTS.md**: Documentation for using forgejo-cli with AI agents.
- **CHANGELOG.md**: This file.

### Changed

- **`repo delete`**: Now uses `--force` flag pattern instead of custom readline prompt. Added `--dry-run` support.
- **`org team delete`**: Now uses `--force` flag pattern instead of custom readline prompt. Added `--dry-run` support.
- **`user gpg delete`**: Added `--dry-run` support (already had `--force`).
- **Editor behavior**: When `--yes` is set and no body content is provided, commands now fail with a helpful error message instead of trying to spawn an editor.
- **Confirmation prompts**: All `prompt_bool()` calls now respect the global `--yes` flag.

### Security

- **Destructive operations now have consistent safeguards**: All delete/remove operations now require either interactive confirmation, `--force`, or `--yes` flag. Previously, some operations (like `actions variables delete`, `release delete`) had no safeguards.

## [0.3.0] - Previous Release

(Previous changelog entries would go here)
