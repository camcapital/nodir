use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use nodir_core::{Piff, SyncMode, SyncOutcome};

mod editor;
mod markdown;
mod tui;

use markdown::strip_controls;

/// Browse and sync Piffs: directories with their own markdown content.
///
/// Every directory is a Parent File (Piff). Its content lives in a PIFF.md
/// inside it, and NoDir keeps a list of its children at the end of that file.
///
/// Run `nodir [path]` without a subcommand to browse Piffs interactively.
#[derive(Parser)]
#[command(
    name = "nodir",
    version,
    about,
    long_about,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// The Piff (directory) to open in the browser.
    path: Option<PathBuf>,
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
        None => tui::run(&cli.path.unwrap_or_else(|| ".".into())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if is_broken_pipe(&err) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nodir: {}", strip_controls(&format!("{err:#}")));
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
        let title = format!("\x1b[1;7m {} \x1b[0m", strip_controls(&piff.name()));
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
    let tty = io::stdout().is_terminal();
    let mut out = io::stdout().lock();
    for child in piff.children()? {
        // File names can contain escape sequences too; only a pipe gets
        // them verbatim.
        let name = if tty {
            strip_controls(&child.name)
        } else {
            child.name.as_str().into()
        };
        match (child.is_piff(), color) {
            (true, true) => writeln!(out, "\x1b[1;34m{name}/\x1b[0m")?,
            (true, false) => writeln!(out, "{name}/")?,
            (false, _) => writeln!(out, "{name}")?,
        }
    }
    out.flush()?;
    Ok(())
}

fn sync(path: &Path, recursive: bool) -> Result<()> {
    let piff = Piff::open(path)?;
    let reports = if recursive {
        piff.sync_recursive(SyncMode::CreateIfMissing)
    } else {
        vec![nodir_core::SyncReport {
            path: piff.content_path(),
            result: piff.sync(SyncMode::CreateIfMissing),
        }]
    };
    let tty = io::stdout().is_terminal();
    let (mut created, mut updated, mut unchanged, mut failed) = (0, 0, 0, 0);
    let mut out = io::stdout().lock();
    for report in &reports {
        let shown = report.path.display().to_string();
        let shown = if tty {
            strip_controls(&shown)
        } else {
            shown.as_str().into()
        };
        match &report.result {
            Ok(SyncOutcome::Created) => {
                created += 1;
                writeln!(out, "created {shown}")?;
            }
            Ok(SyncOutcome::Updated) => {
                updated += 1;
                writeln!(out, "updated {shown}")?;
            }
            Ok(SyncOutcome::Unchanged | SyncOutcome::Skipped) => unchanged += 1,
            Err(err) => {
                failed += 1;
                out.flush()?;
                eprintln!("nodir: {}", strip_controls(&err.to_string()));
            }
        }
    }
    let total = created + updated + unchanged;
    if recursive || created + updated == 0 {
        writeln!(
            out,
            "{total} Piff{} synced: {created} created, {updated} updated, {unchanged} unchanged",
            if total == 1 { "" } else { "s" },
        )?;
    }
    out.flush()?;
    if failed > 0 {
        bail!(
            "{failed} Piff{} could not be synced",
            if failed == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

fn edit(path: &Path) -> Result<()> {
    editor::edit_piff(&Piff::open(path)?, editor::run_editor)
}
