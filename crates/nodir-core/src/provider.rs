//! Where a Piff's content lives.
//!
//! The SLC stores content in `PIFF.md`; other formats can be added later
//! behind the same trait without touching the rest of the model.

use std::path::{Path, PathBuf};

/// Locates a Piff's content file.
pub trait ContentProvider: Send + Sync {
    /// The content file's name inside the Piff's directory, e.g. `PIFF.md`.
    fn file_name(&self) -> &str;

    /// Full path of the content file for the Piff at `dir`.
    fn content_path(&self, dir: &Path) -> PathBuf {
        dir.join(self.file_name())
    }
}

/// The default provider: markdown content in `PIFF.md`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PiffMd;

impl ContentProvider for PiffMd {
    fn file_name(&self) -> &str {
        crate::CONTENT_FILE_NAME
    }
}
