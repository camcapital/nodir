# nodir
A cross-platform file explorer where folders are Parent Files (Piffs): directories with their own markdown content.

## CLI (preview)

```sh
cargo install --path crates/nodir-cli

nodir show [path]       # print a Piff: its PIFF.md plus its children (never writes)
nodir ls [path]         # list children; child Piffs end in /
nodir sync [path] [-r]  # write the children section into PIFF.md (recursively with -r)
nodir edit [path]       # open PIFF.md in $VISUAL/$EDITOR, then sync
```

NoDir owns only the text between `<!-- nodir:children:start -->` and `<!-- nodir:children:end -->` at the end of each `PIFF.md`. Everything else in the file is yours and is never changed. See [`examples/sample-tree`](examples/sample-tree/PIFF.md) for a synced tree you can browse on GitHub.
