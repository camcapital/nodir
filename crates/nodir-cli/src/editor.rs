//! Editing a Piff's content in the user's external editor.

use std::env;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use nodir_core::{Piff, SyncMode};

/// Edits `piff`'s content file with `run_editor`, creating it (seeded with
/// a heading) if it is missing, and regenerates the children section before
/// and after.
///
/// If the user deletes the file in the editor, it is not recreated.
pub fn edit_piff(piff: &Piff, run_editor: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if !piff.has_content() {
        piff.write_content(&format!("# {}\n", piff.name()))?;
    } else {
        piff.sync(SyncMode::ExistingOnly)?;
    }
    run_editor(&piff.content_path())?;
    piff.sync(SyncMode::ExistingOnly)?;
    Ok(())
}

/// The user's editor command: $VISUAL, then $EDITOR, then a platform default.
fn editor() -> String {
    ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|var| env::var(var).ok())
        .find(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "vi".into()
            }
        })
}

/// Runs the editor on `file` and waits for it. The editor is run through
/// the shell, like git does, so values such as `code --wait` or a quoted
/// path with spaces work.
pub fn run_editor(file: &Path) -> Result<()> {
    let editor = editor();
    let status = editor_command(&editor, file)
        .status()
        .with_context(|| format!("could not start editor `{editor}`"))?;
    if !status.success() {
        bail!("editor `{editor}` exited with {status}");
    }
    Ok(())
}

#[cfg(windows)]
fn editor_command(editor: &str, file: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new("cmd");
    // cmd.exe has its own quoting rules: with /S it strips the outer quotes
    // and runs the rest verbatim, so pass the line through raw_arg.
    cmd.args(["/S", "/C"])
        .raw_arg(format!("\"{editor} \"{}\"\"", file.display()));
    cmd
}

#[cfg(not(windows))]
fn editor_command(editor: &str, file: &Path) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg(editor)
        .arg(file);
    cmd
}
