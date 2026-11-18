# Rust crates

The Rust workspace's library and CLI. Every NoDir front end shares `nodir-core`, so Piff behaviour lives there and nowhere else.

- `nodir-core`: the Piff model, the `PIFF.md` content provider, children listing (hidden files, `.git` and `.gitignore` matches excluded) and reading and writing the children section between the `nodir:children` markers. It never touches text outside those markers.
- `nodir-cli`: the `nodir` binary. `nodir [path]` opens the TUI (ratatui); `show`, `ls`, `sync` and `edit` are the scriptable subcommands.

Run `cargo test --workspace` from the repository root to test both.

<!-- nodir:children:start -->
- [nodir-cli/](nodir-cli/PIFF.md)
- [nodir-core/](nodir-core/PIFF.md)
<!-- nodir:children:end -->
