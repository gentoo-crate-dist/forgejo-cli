# Using forgejo-cli with AI Agents

This document describes how AI agents (like Claude, GPT, etc.) can safely and effectively use `forgejo-cli`.

## Quick Start for Agents

**Always use these flags:**
```bash
fj --yes --verbose <command>
```

- `--yes` (-y): Bypasses all interactive prompts
- `--verbose` (-v): Shows detailed logging for debugging
- `--json`: Machine-readable output (when available)

## Safe Patterns

### Preview Before Destructive Actions

Always run with `--dry-run` first to see what would happen:

```bash
# Step 1: Preview
fj repo delete owner/repo --dry-run
# Output: Would delete repository: owner/repo
#         Run with --force to actually delete.

# Step 2: Execute (only if preview looks correct)
fj repo delete owner/repo --force --yes
```

### Handling Multi-Line Content

Use `--body` for inline content or `--body-file -` to read from stdin:

```bash
# Inline body
fj pr create --title "Fix bug" --body "This fixes the bug described in #123"

# Read body from file
fj pr create --title "Feature" --body-file description.md

# Read from stdin (useful for heredocs)
fj issue create --title "Bug report" --body-file - <<EOF
## Description
The application crashes when...

## Steps to Reproduce
1. Open the app
2. Click on...
EOF
```

## Commands Reference

### Destructive Operations

All delete/remove operations support `--force` and `--dry-run`:

| Command | Description |
|---------|-------------|
| `repo delete` | Delete a repository |
| `release delete` | Delete a release |
| `release asset delete` | Delete a release asset |
| `actions variables delete` | Delete a CI/CD variable |
| `actions secrets delete` | Delete a CI/CD secret |
| `org team delete` | Delete a team |
| `org team repo rm` | Remove repo access from team |
| `org team member rm` | Remove user from team |
| `user key delete` | Delete an SSH key |
| `user gpg delete` | Delete a GPG key |

### Example: Safe Deletion Workflow

```bash
# 1. List items to understand current state
fj release list

# 2. Preview the deletion
fj release delete v1.0.0-beta --dry-run

# 3. Execute with force (no prompt) and yes (global override)
fj --yes release delete v1.0.0-beta --force
```

## Error Handling

When errors occur, the CLI provides helpful messages:

```bash
$ fj --yes issue create --title "Bug"
Error: No content provided and --yes flag prevents interactive editor.

Provide content using --body or --body-file flags.
```

**Solution:** Always provide `--body` or `--body-file` when using `--yes`.

## Common Workflows

### Create a PR

```bash
fj pr create \
  --title "feat: add new feature" \
  --body "## Summary\nThis PR adds..." \
  --head feature-branch \
  --base main
```

### Create an Issue

```bash
fj issue create \
  --title "Bug: Application crashes" \
  --body-file - <<EOF
## Description
The application crashes when clicking the submit button.

## Steps to Reproduce
1. Fill out the form
2. Click submit
3. Application crashes
EOF
```

### Manage CI/CD Variables

```bash
# List variables
fj actions variables list

# Create/update a variable (--force to overwrite)
fj actions variables create MY_VAR "my_value" --force

# Delete a variable (safe pattern)
fj actions variables delete MY_VAR --dry-run
fj --yes actions variables delete MY_VAR --force
```

### Clone and Navigate

```bash
# Clone a repository
fj repo clone owner/repo

# Fork a repository
fj repo fork owner/repo

# View repository info
fj repo view owner/repo
```

## Flags Summary

| Flag | Scope | Description |
|------|-------|-------------|
| `--yes` / `-y` | Global | Skip all interactive prompts |
| `--verbose` / `-v` | Global | Verbose debug output |
| `--json` | Global | JSON output format |
| `--host` / `-H` | Global | Specify Forgejo instance |
| `--force` / `-f` | Per-command | Skip confirmation for destructive ops |
| `--dry-run` | Per-command | Preview without executing |
| `--body` | Create commands | Inline content |
| `--body-file` | Create commands | Read content from file (use `-` for stdin) |

## Limitations

1. **JSON output**: Not all commands support `--json` yet. Check individual command help.
2. **Editor fallback**: When `--yes` is set and no body is provided, commands that would normally open an editor will fail with a helpful error message.
3. **Rate limiting**: Be aware of API rate limits when making many requests.
