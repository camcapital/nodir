use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use nodir_core::{Piff, SyncMode, SyncOutcome};

mod markdown;

/// Browse and sync Piffs: directories with their own markdown content.
///
/// Every directory is a Parent File (Piff). Its content lives in a PIFF.md
/// inside it, and NoDir keeps a list of its children at the end of that file.
#[derive(Parser)]
#[command(name = "nodir", version, about, long_about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print a Piff: its content plus an up-to-date children list.
    ///
    /// Styled when stdout is a terminal, raw markdown otherwise. Never writes
    /// anything, even when PIFF.md is missing or stale.
    Show {
        /// The Piff (directory) to show.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// List a Piff's children. Child Piffs end in `/`.
    Ls {
        /// The Piff (directory) to list.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Regenerate the children section in PIFF.md, creating it if missing.
    Sync {
        /// The Piff (directory) to sync.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Also sync every child Piff below it (skips hidden and git-ignored
        /// directories).
        #[arg(short, long)]
        recursive: bool,
    },
    /// Open PIFF.md in $VISUAL or $EDITOR (creating it if needed), then sync.
    Edit {
        /// The Piff (directory) to edit.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Cmd::Show { path }) => show(&path),
        Some(Cmd::Ls { path }) => ls(&path),
        Some(Cmd::Sync { path, recursive }) => sync(&path, recursive),
        Some(Cmd::Edit { path }) => edit(&path),
        // `nodir [path]` opens the TUI browser (CAMC-322). Until then, help.
        None => {
            use clap::CommandFactory;
            Cli::command().print_long_help().map_err(Into::into)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if is_broken_pipe(&err) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nodir: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
}

/// Whether to style stdout: a terminal, and NO_COLOR unset.
fn styled() -> bool {
    io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

fn show(path: &Path) -> Result<()> {
    let piff = Piff::open(path)?;
    let content = piff.render()?;
    let mut out = io::stdout().lock();
    if styled() {
        let title = format!("\x1b[1;7m {} \x1b[0m", piff.name());
        writeln!(out, "{title}")?;
        if !piff.has_content() {
            writeln!(
                out,
                "\x1b[2mEmpty Piff (no {})\x1b[0m",
                piff.content_file_name()
            )?;
        }
        writeln!(out)?;
        out.write_all(markdown::to_ansi(&content).as_bytes())?;
    } else {
        out.write_all(content.as_bytes())?;
    }
    out.flush()?;
    Ok(())
}

fn ls(path: &Path) -> Result<()> {
    let piff = Piff::open(path)?;
    let color = styled();
    let mut out = io::stdout().lock();
    for child in piff.children()? {
        match (child.is_piff(), color) {
            (true, true) => writeln!(out, "\x1b[1;34m{}/\x1b[0m", child.name)?,
            (true, false) => writeln!(out, "{}/", child.name)?,
            (false, _) => writeln!(out, "{}", child.name)?,
        }
    }
    out.flush()?;
    Ok(())
}

fn sync(path: &Path, recursive: bool) -> Result<()> {
    let piff = Piff::open(path)?;
    let reports = if recursive {
        piff.sync_recursive(SyncMode::CreateIfMissing)?
    } else {
        vec![nodir_core::SyncReport {
            path: piff.content_path(),
            outcome: piff.sync(SyncMode::CreateIfMissing)?,
        }]
    };
    let (mut created, mut updated, mut unchanged) = (0, 0, 0);
    let mut out = io::stdout().lock();
    for report in &reports {
        match report.outcome {
            SyncOutcome::Created => {
                created += 1;
                writeln!(out, "created {}", report.path.display())?;
            }
            SyncOutcome::Updated => {
                updated += 1;
                writeln!(out, "updated {}", report.path.display())?;
            }
            SyncOutcome::Unchanged | SyncOutcome::Skipped => unchanged += 1,
        }
    }
    if recursive || created + updated == 0 {
        writeln!(
            out,
            "{} Piff{} synced: {created} created, {updated} updated, {unchanged} unchanged",
            reports.len(),
            if reports.len() == 1 { "" } else { "s" },
        )?;
    }
    out.flush()?;
    Ok(())
}

fn edit(path: &Path) -> Result<()> {
    let piff = Piff::open(path)?;
    if !piff.has_content() {
        piff.write_content(&format!("# {}\n", piff.name()))?;
    } else {
        piff.sync(SyncMode::ExistingOnly)?;
    }
    let file = piff.content_path();
    run_editor(&file)?;
    // The user may have deleted the file in the editor; don't recreate it.
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

/// Runs the editor through the shell, like git does, so values such as
/// `code --wait` or a quoted path with spaces work.
fn run_editor(file: &Path) -> Result<()> {
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
