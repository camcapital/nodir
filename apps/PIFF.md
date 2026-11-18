# Apps

Graphical front ends for NoDir.

- `desktop`: the desktop app for macOS, Windows and Linux, built with Tauri v2. The frontend in `src/` is TypeScript with a CodeMirror 6 live-preview editor; the Rust side in `src-tauri/` calls `nodir-core` for everything to do with Piffs.

Run it from `apps/desktop` with `npm install` and then `npm run tauri dev`.

<!-- nodir:children:start -->
- [desktop/](desktop/PIFF.md)
<!-- nodir:children:end -->
