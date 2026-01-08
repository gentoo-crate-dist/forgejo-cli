# forgejo-cli

CLI tool for interacting with Forgejo

[Matrix Chat](https://matrix.to/#/#forgejo-cli:cartoon-aa.xyz)

## Installation

### Pre-built

Pre-built binaries are available for `x86_64` Windows and Linux (GNU) on the
[releases tab](https://codeberg.org/forgejo-contrib/forgejo-cli/releases/latest).

### From source

Install with `cargo install` or `cargo binstall`

```
# Latest version compiled from source
cargo install forgejo-cli
# From `main`
cargo install --git https://codeberg.org/forgejo-contrib/forgejo-cli.git --branch main

# Using precompiled releases (requires cargo-binstall)
cargo binstall forgejo-cli
```

### Debian and Ubuntu

forgejo-cli is included in the official repositories as of Debian unstable and Ubuntu plucky.
```sh
sudo apt install forgejo-cli
```

### Fedora

An **unofficial** [COPR repository](https://copr.fedorainfracloud.org/coprs/lihaohong/forgejo-cli/) is available. Fedora users can install forgejo-cli with
```sh
sudo dnf copr enable lihaohong/forgejo-cli
sudo dnf install forgejo-cli
```

If you see an error about copr not being an available command, run `sudo dnf install dnf-plugins-core`.

### Guix

If it's not already done, you need to update Guix (Guix is rolling release):
```sh
guix pull
```

One done you can then install forgejo-cli with the following command:
```sh
guix install forgejo-cli
```

### Nix

A Nix flake is included in this repository that you may use. You could install it into your Nix
profile, for example:
```
nix profile install git+https://codeberg.org/forgejo-contrib/forgejo-cli
```
...or include it in the flake inputs of your NixOS system:
```nix
{
  inputs = {
    # ...
    forgejo-cli.url = "git+https://codeberg.org/forgejo-contrib/forgejo-cli";
  };
  # ...
}
```


### OCI Container

`forgejo-cli` is available as an OCI container for use in CI, at
`codeberg.org/cyborus/forgejo-cli:latest`

## Usage

### Global Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Skip all interactive prompts (answer "yes" automatically) |
| `--json` | | Output in JSON format for machine parsing |
| `--verbose` | `-v` | Enable verbose output for debugging |
| `--host` | `-H` | Specify the Forgejo instance to use |

### Non-Interactive Mode

For use in scripts or with AI agents, use `--yes` to bypass all confirmation prompts:

```bash
# Delete without interactive confirmation
fj repo delete owner/repo --force --yes

# Preview what would be deleted (safe)
fj release delete v1.0.0 --dry-run

# Combine flags for scripting
fj --yes actions variables delete MY_VAR --force
```

### Destructive Operations

All destructive operations (delete, remove) support:
- `--dry-run`: Preview what would happen without making changes
- `--force` / `-f`: Skip confirmation prompts (also bypassed by global `--yes`)

```bash
# Preview first
fj repo delete owner/repo --dry-run
# Then execute
fj repo delete owner/repo --force
```

### Instance-specific aliases

While you can just use the `fj` binary directly, it can be useful to alias it
with the `--host` flag set, to create shorthands for certain instances.

```bash
# For example, a `cb` command for interacting with codeberg
alias cb="fj --host codeberg.org"
# Or code.forgejo.org
alias cfj="fj --host code.forgejo.org"
# Or any other instance you want!
# And the alias name can be whatever, as long as the `--host` flag is set.
```

Now, when you reference a repository such as `forgejo/forgejo`, it will
implicitly get it from whichever alias you used!

```
$ cb repo view forgejo/forgejo
forgejo/forgejo
> Beyond coding. We forge.

Primary language is Go
# etc...
```

When using `fj` directly, you'd have to use a URL to access it.

```
$ fj repo view codeberg.org/forgejo/forgejo
forgejo/forgejo
> Beyond coding. We forge.

Primary language is Go
# etc...

# Notice the "cfj", trying to access code.forgejo.org, still works when you specify Codeberg in the repository name!
$ cfj repo view codeberg.org/forgejo/forgejo
forgejo/forgejo
> Beyond coding. We forge.

Primary language is Go
# etc...
```

## Licensing

This project is licensed under either
[Apache License Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

