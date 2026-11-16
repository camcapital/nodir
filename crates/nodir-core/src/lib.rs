//! Shared Piff model for NoDir.
//!
//! Every directory is a Parent File (Piff): it holds files like a folder does
//! and carries its own markdown content in a `PIFF.md` file.

/// File name of a Piff's markdown content.
pub const CONTENT_FILE_NAME: &str = "PIFF.md";
