//! The Piff: a directory plus its markdown content.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::children::{self, Child};
use crate::provider::{ContentProvider, PiffMd};
use crate::section;
use crate::{Error, Result};

/// Whether a sync may create a missing content file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Regenerate the children section only where the content file already
    /// exists. Use this when a Piff is merely opened or browsed.
    ExistingOnly,
    /// Create the content file if it is missing. Use this only when the user
    /// explicitly asks for a sync or writes content.
    CreateIfMissing,
}

/// What a sync did to one Piff's content file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The content file did not exist and was created.
    Created,
    /// The children section changed and the file was rewritten.
    Updated,
    /// The file was already up to date; it was not written.
    Unchanged,
    /// The file does not exist and the mode did not allow creating it.
    Skipped,
}

/// The result of syncing one Piff during a recursive sync.
#[derive(Debug)]
pub struct SyncReport {
    /// The Piff's content file path, or its directory when listing its
    /// children failed.
    pub path: PathBuf,
    /// What happened to it, or why it failed.
    pub result: Result<SyncOutcome>,
}

/// A directory viewed as a Parent File.
///
/// Reading a Piff never writes anything: only [`Piff::sync`],
/// [`Piff::sync_recursive`] and [`Piff::write_content`] touch the disk.
#[derive(Clone)]
pub struct Piff {
    dir: PathBuf,
    provider: &'static dyn ContentProvider,
}

impl std::fmt::Debug for Piff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Piff")
            .field("dir", &self.dir)
            .field("content_file", &self.provider.file_name())
            .finish()
    }
}

impl Piff {
    /// Opens the Piff at `dir` with the default `PIFF.md` provider.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        Self::with_provider(dir, &PiffMd)
    }

    /// Opens the Piff at `dir` with a custom content provider.
    pub fn with_provider(
        dir: impl Into<PathBuf>,
        provider: &'static dyn ContentProvider,
    ) -> Result<Self> {
        let dir = dir.into();
        if !dir.is_dir() {
            return Err(Error::NotADirectory(dir));
        }
        Ok(Self { dir, provider })
    }

    /// The Piff's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory's name, or the full path for a root such as `/` or `C:\`.
    pub fn name(&self) -> String {
        match self.dir.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => self.dir.display().to_string(),
        }
    }

    /// Path of the content file, whether or not it exists.
    pub fn content_path(&self) -> PathBuf {
        self.provider.content_path(&self.dir)
    }

    /// The content file's name, e.g. `PIFF.md`.
    pub fn content_file_name(&self) -> &str {
        self.provider.file_name()
    }

    /// Whether the content file exists. A Piff without one is empty.
    pub fn has_content(&self) -> bool {
        self.content_path().is_file()
    }

    /// The content file's text as stored on disk, or `None` if it is missing.
    pub fn read(&self) -> Result<Option<String>> {
        let path = self.content_path();
        match fs::read(&path) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Error::NotUtf8(path)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(Error::io(path, err)),
        }
    }

    /// The Piff's immediate children (see [`children::list`]).
    pub fn children(&self) -> Result<Vec<Child>> {
        children::list(&self.dir, self.provider.file_name())
    }

    /// The Piff's content with an up-to-date children section, computed in
    /// memory. Nothing is written, even if the content file is missing or
    /// its section is stale.
    pub fn render(&self) -> Result<String> {
        let content = self.read()?.unwrap_or_default();
        self.apply(&content)
    }

    /// Regenerates the children section in the content file.
    ///
    /// The file is only written when its bytes would change, and only
    /// created when `mode` is [`SyncMode::CreateIfMissing`].
    pub fn sync(&self, mode: SyncMode) -> Result<SyncOutcome> {
        let current = self.read()?;
        if current.is_none() && mode == SyncMode::ExistingOnly {
            return Ok(SyncOutcome::Skipped);
        }
        let old = current.as_deref().unwrap_or_default();
        let new = self.apply(old)?;
        if current.is_some() && new == old {
            return Ok(SyncOutcome::Unchanged);
        }
        self.write_raw(&new)?;
        Ok(if current.is_some() {
            SyncOutcome::Updated
        } else {
            SyncOutcome::Created
        })
    }

    /// Syncs this Piff and every Piff below it, depth-first, parents before
    /// children. Descends only into child Piffs that are listed as children
    /// (so hidden and git-ignored directories are skipped) and never follows
    /// symlinks.
    pub fn sync_recursive(&self, mode: SyncMode) -> Vec<SyncReport> {
        let mut reports = Vec::new();
        self.sync_into(mode, &mut reports);
        reports
    }

    /// Errors are recorded and the walk carries on, so one bad Piff (say, a
    /// non-UTF-8 `PIFF.md`) doesn't leave the rest of the tree half-synced.
    fn sync_into(&self, mode: SyncMode, reports: &mut Vec<SyncReport>) {
        reports.push(SyncReport {
            path: self.content_path(),
            result: self.sync(mode),
        });
        let children = match self.children() {
            Ok(children) => children,
            Err(err) => {
                reports.push(SyncReport {
                    path: self.dir.clone(),
                    result: Err(err),
                });
                return;
            }
        };
        for child in children {
            if !child.is_piff() || child.path.is_symlink() {
                continue;
            }
            match Piff::with_provider(&child.path, self.provider) {
                Ok(piff) => piff.sync_into(mode, reports),
                Err(err) => reports.push(SyncReport {
                    path: child.path,
                    result: Err(err),
                }),
            }
        }
    }

    /// Replaces the content with `text`, then regenerates the children
    /// section, creating the content file if needed. If `text` contains a
    /// children section, it is regenerated in place; otherwise one is
    /// appended.
    pub fn write_content(&self, text: &str) -> Result<SyncOutcome> {
        let existed = self.has_content();
        let new = self.apply(text)?;
        if existed && self.read()?.as_deref() == Some(new.as_str()) {
            return Ok(SyncOutcome::Unchanged);
        }
        self.write_raw(&new)?;
        Ok(if existed {
            SyncOutcome::Updated
        } else {
            SyncOutcome::Created
        })
    }

    fn apply(&self, content: &str) -> Result<String> {
        let children = self.children()?;
        Ok(section::apply(
            content,
            &children,
            self.provider.file_name(),
        ))
    }

    fn write_raw(&self, text: &str) -> Result<()> {
        let path = self.content_path();
        // A symlinked content file is written through to its target rather
        // than replaced by a regular file.
        let target = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        atomic_write(&target, text.as_bytes()).map_err(|err| Error::io(path, err))
    }
}

/// Writes `bytes` to a hidden temporary file next to `path`, then renames it
/// over `path`, so a crash mid-write never leaves the user's file truncated.
/// The existing file's permissions are kept.
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.nodir-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
