//! Listing a Piff's immediate children.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

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
/// Excluded: hidden entries (dot-files, `.git`, and on Windows anything with
/// the hidden attribute), the content file itself (`content_file`, e.g.
/// `PIFF.md`), and anything matched by `.gitignore` rules. The rules come
/// from `dir`'s own `.gitignore` and, when `dir` is inside a git repository,
/// from every `.gitignore` between the repository root and `dir`. Outside a
/// repository no parent `.gitignore` is read, and global or repository-local
/// excludes (`core.excludesFile`, `.git/info/exclude`) are never applied, so
/// the list, and the children section written from it, is the same on every
/// machine.
///
/// A child is judged by its own name only: opening an ignored directory
/// directly still lists its contents.
pub fn list(dir: &Path, content_file: &str) -> Result<Vec<Child>> {
    if !dir.is_dir() {
        return Err(Error::NotADirectory(dir.to_path_buf()));
    }
    let real_dir = dir.canonicalize().map_err(|e| Error::io(dir, e))?;
    let ignores = gitignores(&real_dir);

    let mut children = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == content_file || is_hidden_attr(&entry) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| Error::io(&path, e))?;
        let is_dir = if file_type.is_symlink() {
            fs::metadata(&path).is_ok_and(|m| m.is_dir())
        } else {
            file_type.is_dir()
        };
        if is_ignored(&ignores, &real_dir.join(&name), is_dir) {
            continue;
        }
        children.push(Child {
            name,
            path,
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

/// The `.gitignore` matchers that apply to `dir`'s children, deepest first.
fn gitignores(dir: &Path) -> Vec<Gitignore> {
    let repo_root = dir.ancestors().find(|a| a.join(".git").exists());
    let mut dirs: Vec<&Path> = match repo_root {
        Some(root) => dir.ancestors().take_while(|a| *a != root).collect(),
        None => vec![dir],
    };
    if let Some(root) = repo_root {
        dirs.push(root);
    }
    dirs.into_iter()
        .filter_map(|d| {
            let file = d.join(".gitignore");
            if !file.is_file() {
                return None;
            }
            let mut builder = GitignoreBuilder::new(d);
            // A malformed line is skipped, like git does.
            let _ = builder.add(&file);
            builder.build().ok()
        })
        .collect()
}

/// Git semantics: the deepest `.gitignore` with a matching rule decides, and
/// a `!` rule re-includes.
fn is_ignored(ignores: &[Gitignore], path: &Path, is_dir: bool) -> bool {
    for gi in ignores {
        match gi.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

#[cfg(windows)]
fn is_hidden_attr(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    entry
        .metadata()
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
}

#[cfg(not(windows))]
fn is_hidden_attr(_entry: &fs::DirEntry) -> bool {
    false
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
