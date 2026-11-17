# nodir
A cross-platform file explorer where folders are Parent Files (Piffs): directories with their own markdown content.

## CLI (preview)

```sh
cargo install --path crates/nodir-cli

nodir [path]            # browse Piffs in the terminal
nodir show [path]       # print a Piff: its PIFF.md plus its children (never writes)
nodir ls [path]         # list children; child Piffs end in /
nodir sync [path] [-r]  # write the children section into PIFF.md (recursively with -r)
nodir edit [path]       # open PIFF.md in $VISUAL/$EDITOR, then sync
```

`nodir [path]` opens a browser: the Piff's children on the left, its rendered `PIFF.md` on the right. Use ↑/↓ (or `j`/`k`) to pick a child, Enter/→ to open it (a file opens in your editor), ←/Backspace to go to the parent, PgUp/PgDn (or Space/`b`, Ctrl-d/Ctrl-u) to scroll, `e` to edit the Piff in `$VISUAL`/`$EDITOR`, `r` to reload and `q` to quit. Opening a Piff refreshes the children section of its existing `PIFF.md`; browsing never creates one.

NoDir owns only the text between `<!-- nodir:children:start -->` and `<!-- nodir:children:end -->` at the end of each `PIFF.md`. Everything else in the file is yours and is never changed. See [`examples/sample-tree`](examples/sample-tree/PIFF.md) for a synced tree you can browse on GitHub.
