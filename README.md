## `fj`, a CLI client for Forgejo

... akin to `gh`, `glab`, or `tea`!

`fj` doesn't try to replace your usage of `git`, it's meant to work alongside it.
It handles all the Forgejo-specific things that `git` doesn't.

[Matrix Chat](https://matrix.to/#/#forgejo-cli:cartoon-aa.xyz)

### Getting Started

1. [Install `fj`](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Installation)
2. Authenticate with your instance:
   ```
   fj auth login
   ```
3. Clone a repo and start working:
   ```
   fj repo clone owner/repo
   cd repo
   ```

### Common Usage

```sh
# Issues
fj issue create "Bug report title"
fj issue search --state open
fj issue view 42

# Pull Requests
fj pr create "My feature"
fj pr search --state open
fj pr view 10
fj pr merge 10

# Repositories
fj repo create my-project
fj repo view
fj repo readme

# Releases & Tags
fj release create v1.0.0
fj tag list
```

### Documentation

See [the wiki](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki) for full documentation:

- [Installation](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Installation) / [Building from source](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Building-from-source)
- [Authentication](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Authentication)
- [Repositories](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Repositories)
- [Issues](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Issues)
- [Pull Requests](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/PRs)
- [Actions](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Actions)
- [Users](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Users)
- [Organizations](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Organizations)

### Installation

Pre-built binaries are available for `x86_64` Windows and Linux (GNU) on the
[releases tab](https://codeberg.org/forgejo-contrib/forgejo-cli/releases/latest).

See [the wiki page on installation](https://codeberg.org/forgejo-contrib/forgejo-cli/wiki/Installation) for more options.

### Licensing

This project is licensed under either
[Apache License Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
