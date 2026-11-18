# NoDir

A file explorer for macOS, Windows and Linux in which every folder is a **Parent File**, or **Piff**: it holds files like a folder does, and it has its own markdown content too.

![The NoDir desktop app editing a Piff with live preview](docs/demo/live-preview.gif)

## Why Piffs?

A folder's name rarely tells you why the folder exists, what state it is in, or what you should do with what is inside. A Piff can say so. Write it down once and anyone who opens the folder sees it, whether that's you, a teammate or an AI agent:

> Drafts for the Q3 launch. Final copy lives in `../published`.
> DO NOT TRUST THE CONTENTS OF THIS PIFF: it is a scratch area.

NoDir is a layer over ordinary files and directories. It never restructures the file system.

- A Piff's content lives in a visible `PIFF.md` inside the directory. A directory without one is an empty Piff.
- `PIFF.md` is created only when you write content or explicitly sync. Browsing a folder never adds files to it.
- At the end of each `PIFF.md`, NoDir keeps a list of the Piff's immediate children between `<!-- nodir:children:start -->` and `<!-- nodir:children:end -->`. Hidden files, `.git` and anything in `.gitignore` are left out. The links are relative, and a child Piff links to its own `PIFF.md`, so you can browse the tree on GitHub too.
- NoDir owns only the text between those two markers. Everything else in the file is yours and is never changed.

Your folders keep working as usual in Finder, Explorer, your editor, git and GitHub. This repository is itself a tree of Piffs: start at [`PIFF.md`](PIFF.md), or browse [`examples/sample-tree`](examples/sample-tree/PIFF.md).

## Install

Download the files for your system from the [latest release](https://github.com/camcapital/nodir/releases/latest). Each release includes the desktop app and the `nodir` CLI, and `SHA256SUMS.txt` lists every file's checksum.

| System | Desktop app | CLI |
|---|---|---|
| macOS (Apple silicon and Intel) | `NoDir_<version>_universal.dmg` | `nodir-<version>-macos-universal.tar.gz` |
| Windows (x64) | `NoDir_<version>_x64-setup.exe` or `NoDir_<version>_x64_en-US.msi` | `nodir-<version>-windows-x86_64.zip` |
| Linux (x64) | `NoDir_<version>_amd64.AppImage` or `NoDir_<version>_amd64.deb` | `nodir-<version>-linux-x86_64.tar.gz` |
| Linux (arm64) | | `nodir-<version>-linux-aarch64.tar.gz` |

The builds are not signed or notarized yet, so macOS and Windows warn you the first time you open them. The steps below get past the warning.

### macOS

**Desktop app**

1. Open the `.dmg` and drag **NoDir** into **Applications**.
2. Open NoDir. macOS says it can't verify the developer; click **Done** (or **Cancel**).
3. Open **System Settings → Privacy & Security**, scroll to **Security**, and click **Open Anyway** next to the message about NoDir. Confirm with your password.

You only do this once. If macOS instead says the app "is damaged and can't be opened", remove the download quarantine flag and open it again:

```sh
xattr -dr com.apple.quarantine /Applications/NoDir.app
```

**CLI**

```sh
tar -xzf nodir-*-macos-universal.tar.gz
xattr -d com.apple.quarantine nodir 2>/dev/null  # only needed if a browser downloaded it
sudo mv nodir /usr/local/bin/
nodir --version
```

### Windows

**Desktop app:** run the `-setup.exe` (installs for the current user) or the `.msi`. If Windows SmartScreen says "Windows protected your PC", click **More info**, then **Run anyway**.

**CLI:** extract `nodir.exe` from the `.zip` into a folder on your `PATH`. If Windows blocks it, right-click the `.zip` before extracting, choose **Properties**, tick **Unblock**, and click **OK**. Or, in PowerShell:

```powershell
Unblock-File .\nodir-*-windows-x86_64.zip
```

### Linux

**Desktop app:** install the `.deb` on Debian or Ubuntu:

```sh
sudo apt install ./NoDir_*_amd64.deb
```

Or run the AppImage on any distribution:

```sh
chmod +x NoDir_*_amd64.AppImage
./NoDir_*_amd64.AppImage
```

If the AppImage complains about FUSE, install `libfuse2` (`libfuse2t64` on Ubuntu 24.04 and later).

**CLI:** the Linux builds are static, so they run on any distribution.

```sh
tar -xzf nodir-*-linux-x86_64.tar.gz
sudo mv nodir /usr/local/bin/
```

### From source

You need Rust (the version is pinned in `rust-toolchain.toml`) and, for the desktop app, Node.js 22 and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
cargo install --path crates/nodir-cli   # the nodir CLI

cd apps/desktop                         # the desktop app
npm install
npm run tauri dev                       # or: npm run tauri build
```

## Desktop app

- **Open a folder** to use as the root, then move through it with the sidebar tree, the breadcrumb, the back and forward buttons, or the links in each Piff.
- **Edit in place.** A Piff's content is one live-preview editor: markdown renders as you type, and the raw syntax shows only where your cursor is. Changes save automatically.
- **Open in editor** opens the `PIFF.md` or file in your default app. NoDir reloads it when it changes on disk.
- **Create, rename and delete** Piffs and files from the tree's right-click menu or with shortcuts: ⌘/Ctrl+N for a new file, ⌘/Ctrl+Shift+N for a new Piff, F2 to rename, and ⌘⌫ (macOS) or Del to delete. Deleted items go to the Trash or Recycle Bin, so you can restore them as usual (for example with Finder's **Put Back**).

Opening a Piff refreshes the children section of its `PIFF.md` if that file exists. Nothing is created just because you browsed.

## CLI and TUI

```sh
nodir [path]            # browse Piffs in the terminal
nodir show [path]       # print a Piff: its PIFF.md plus its children (never writes)
nodir ls [path]         # list children; child Piffs end in /
nodir sync [path] [-r]  # write the children section into PIFF.md (recursively with -r)
nodir edit [path]       # open PIFF.md in $VISUAL/$EDITOR, creating it if needed, then sync
```

`path` defaults to the current directory. `nodir show` prints styled output in a terminal and plain markdown when piped, so it works in scripts and for agents.

`nodir [path]` opens a browser with the Piff's children on the left and its rendered `PIFF.md` on the right:

| Key | Action |
|---|---|
| ↑ / ↓, `j` / `k` | Select a child |
| Enter, →, `l` | Open the selected Piff (a file opens in your editor) |
| ←, Backspace, `h` | Go to the parent Piff |
| PgUp / PgDn, `b` / Space, Ctrl-u / Ctrl-d | Scroll the content |
| `e` | Edit this Piff in `$VISUAL`/`$EDITOR`, then sync and re-render |
| `r` | Reload |
| `q`, Esc | Quit |

Opening a Piff in the TUI refreshes the children section of its existing `PIFF.md`; browsing never creates one.

## Project layout

| Path | What it is |
|---|---|
| [`crates/nodir-core`](crates/PIFF.md) | The shared Rust library: the Piff model, children listing and the children section |
| [`crates/nodir-cli`](crates/PIFF.md) | The `nodir` CLI and TUI (ratatui) |
| [`apps/desktop`](apps/PIFF.md) | The desktop app (Tauri v2, CodeMirror 6) |
| [`examples/sample-tree`](examples/sample-tree/PIFF.md) | A small synced tree to try NoDir on |

Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml) when a `v*` tag is pushed.

## License

[MIT](LICENSE)
