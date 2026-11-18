//! Shared Piff model for NoDir.
//!
//! Every directory is a Parent File (Piff): it holds files like a folder does
//! and carries its own markdown content in a `PIFF.md` file. NoDir is a layer
//! over ordinary files and directories; it never restructures the tree.
//!
//! - [`Piff`] reads a Piff's content and children and syncs its children
//!   section. Reading never creates `PIFF.md`.
//! - [`children`] lists a directory's immediate children, skipping hidden and
//!   git-ignored entries.
//! - [`section`] writes, regenerates and parses the NoDir-owned block between
//!   [`section::START_MARKER`] and [`section::END_MARKER`], leaving every byte
//!   outside it untouched.
//! - [`ops`] creates, renames and trashes Piffs and files, keeping the
//!   parent's children section up to date.
//! - [`provider`] locates a Piff's content file.

use std::io;
use std::path::PathBuf;

pub mod children;
pub mod ops;
mod piff;
pub mod provider;
pub mod section;

pub use children::{Child, ChildKind};
pub use piff::{Piff, SyncMode, SyncOutcome, SyncReport};
pub use provider::{ContentProvider, PiffMd};

/// File name of a Piff's markdown content.
pub const CONTENT_FILE_NAME: &str = "PIFF.md";

/// Errors from reading or syncing Piffs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The path is not a directory, so it cannot be a Piff.
    #[error("{} is not a directory (only directories are Piffs)", .0.display())]
    NotADirectory(PathBuf),
    /// A name (or path) cannot be used for a new or renamed entry.
    #[error("{0:?}: {1}")]
    InvalidName(String, String),
    /// The content file is not valid UTF-8; NoDir refuses to rewrite it.
    #[error("{} is not valid UTF-8", .0.display())]
    NotUtf8(PathBuf),
    /// An I/O error on the given path.
    #[error("{}: {source}", path.display())]
    Io {
        /// The path being read or written.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
