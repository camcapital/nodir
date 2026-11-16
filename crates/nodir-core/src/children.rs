//! Listing a Piff's immediate children.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::{Error, Result};

/// Whether a child is itself a Piff (a directory) or an ordinary file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildKind {
    /// A directory. Every directory is a Piff.
    Piff,
    /// Anything that is not a directory.
    File,
}

/// One immediate child of a Piff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    /// The file or directory name.
    pub name: String,
    /// Full path to the child.
    pub path: PathBuf,
    /// Piff or file. A symlink is classified by what it points to.
    pub kind: ChildKind,
}

impl Child {
    /// Whether this child is a Piff.
    pub fn is_piff(&self) -> bool {
        self.kind == ChildKind::Piff
    }
}

/// Lists the immediate children of `dir`, Piffs first, then files, each
/// group sorted by name (case-insensitive).
///
/// Excluded: hidden entries (including `.git`), anything matched by a
/// `.gitignore` in `dir` or its parents, and the content file itself
/// (`content_file`, e.g. `PIFF.md`). Global and repository-local excludes
/// (`core.excludesFile`, `.git/info/exclude`) are not applied, so the list,
/// and the children section written from it, is the same on every machine.
pub fn list(dir: &Path, content_file: &str) -> Result<Vec<Child>> {
    if !dir.is_dir() {
        return Err(Error::NotADirectory(dir.to_path_buf()));
    }
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(true)
        .parents(true)
        .ignore(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .build();

    let mut children = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => match err.into_io_error() {
                Some(io) => return Err(Error::io(dir, io)),
                // A malformed .gitignore line is skipped, like git does.
                None => continue,
            },
        };
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == content_file {
            continue;
        }
        let is_dir = match entry.file_type() {
            Some(t) if t.is_symlink() => fs::metadata(entry.path()).is_ok_and(|m| m.is_dir()),
            Some(t) => t.is_dir(),
            None => false,
        };
        children.push(Child {
            name,
            path: entry.into_path(),
            kind: if is_dir {
                ChildKind::Piff
            } else {
                ChildKind::File
            },
        });
    }
    children.sort_by(compare);
    Ok(children)
}

fn compare(a: &Child, b: &Child) -> Ordering {
    let rank = |c: &Child| match c.kind {
        ChildKind::Piff => 0,
        ChildKind::File => 1,
    };
    rank(a)
        .cmp(&rank(b))
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}
