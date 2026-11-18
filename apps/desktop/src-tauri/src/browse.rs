//! What the desktop app shows: Piffs, files, breadcrumbs and link targets,
//! all confined to one root folder.
//!
//! Every Piff operation goes through `nodir-core`. This module only adds the
//! app's view of it: keeping paths inside the root the user opened, turning
//! content into HTML, and working out where a clicked link points.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use nodir_core::ops::{self, Changed};
use nodir_core::{Child, ChildKind, Piff, SyncMode, CONTENT_FILE_NAME};
use percent_encoding::percent_decode_str;
use serde::Serialize;

use crate::edit::{self, PiffDoc};
use crate::render;

/// Files larger than this are not loaded into the main pane.
pub const MAX_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

/// An error shown to the user as text.
pub type Result<T> = std::result::Result<T, String>;

/// The folder the user opened. Every path the app reads must be inside it.
#[derive(Debug, Clone)]
pub struct Root {
    path: PathBuf,
}

impl Root {
    /// Opens `path` as the root. It must be a directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        // `dunce` avoids Windows `\\?\` paths, which users should never see.
        let path = dunce::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !path.is_dir() {
            return Err(format!("{} is not a folder", path.display()));
        }
        Ok(Self { path })
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn info(&self) -> RootInfo {
        RootInfo {
            path: display(&self.path),
            name: piff_name(&self.path),
        }
    }

    /// `path`, normalized, if it lies inside the root.
    ///
    /// The check is lexical: `..` can never climb out of the root, but
    /// symlinks are not resolved. `nodir-core` lists a symlinked directory as
    /// a Piff, so a symlink inside the root that points outside it can be
    /// browsed and read, and opening it syncs that Piff's existing `PIFF.md`
    /// through the link. That is deliberate, since the user chose to open it,
    /// but it means confinement covers path syntax, not the file system.
    /// (`Piff::sync_recursive`, by contrast, never follows symlinks.)
    pub fn contain(&self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.path.join(path)
        };
        let normal = normalize(&joined);
        if normal.starts_with(&self.path) {
            Ok(normal)
        } else {
            Err(format!(
                "{} is outside {}",
                path.display(),
                self.path.display()
            ))
        }
    }

    /// The crumbs from the root down to `path`, inclusive.
    pub fn breadcrumb(&self, path: &Path) -> Vec<Crumb> {
        let mut crumbs = vec![Crumb {
            name: piff_name(&self.path),
            path: display(&self.path),
        }];
        let Ok(rest) = path.strip_prefix(&self.path) else {
            return crumbs;
        };
        let mut current = self.path.clone();
        for part in rest.components() {
            current.push(part);
            crumbs.push(Crumb {
                name: part.as_os_str().to_string_lossy().into_owned(),
                path: display(&current),
            });
        }
        crumbs
    }

    /// The immediate children of the Piff at `path`, as `nodir-core` lists
    /// them.
    pub fn children(&self, path: &str) -> Result<Vec<Entry>> {
        let dir = self.contain(path)?;
        let piff = Piff::open(&dir).map_err(|e| e.to_string())?;
        let children = piff.children().map_err(|e| e.to_string())?;
        Ok(children.into_iter().map(Entry::from).collect())
    }

    /// The Piff at `path`, which must be inside the root.
    pub fn piff(&self, path: &str) -> Result<Piff> {
        let dir = self.contain(path)?;
        Piff::open(&dir).map_err(|e| e.to_string())
    }

    /// Opens the Piff at `path` for editing. Its children section is
    /// regenerated only if `PIFF.md` already exists; a Piff without one
    /// starts as an empty document and nothing is written.
    pub fn open_piff(&self, path: &str) -> Result<PiffView> {
        let piff = self.piff(path)?;
        let dir = piff.dir().to_path_buf();
        let warning = piff
            .sync(SyncMode::ExistingOnly)
            .err()
            .map(|e| format!("Could not update the children section: {e}"));
        Ok(PiffView {
            path: display(&dir),
            name: piff_name(&dir),
            doc: edit::read(&piff)?,
            breadcrumb: self.breadcrumb(&dir),
            warning,
        })
    }

    /// Opens a plain file for reading: markdown is rendered, other UTF-8
    /// text is shown as is, and anything else is left to the OS.
    pub fn open_file(&self, path: &str) -> Result<FileView> {
        let file = self.contain(path)?;
        let meta = fs::metadata(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if meta.is_dir() {
            return Err(format!("{} is a Piff, not a file", file.display()));
        }
        let content = if meta.len() > MAX_PREVIEW_BYTES {
            FileContent::TooLarge { size: meta.len() }
        } else {
            let mut bytes = Vec::with_capacity(meta.len() as usize);
            fs::File::open(&file)
                .and_then(|mut f| f.read_to_end(&mut bytes))
                .map_err(|e| format!("{}: {e}", file.display()))?;
            match String::from_utf8(bytes) {
                Ok(text) if !text.contains('\0') => {
                    if is_markdown(&file) {
                        FileContent::Markdown {
                            html: render::markdown(&text),
                        }
                    } else {
                        FileContent::Text { text }
                    }
                }
                _ => FileContent::Binary { size: meta.len() },
            }
        };
        Ok(FileView {
            path: display(&file),
            name: piff_name(&file),
            breadcrumb: self.breadcrumb(&file),
            content,
        })
    }

    /// Where a link in the item at `from` points. `from` is a Piff directory
    /// or a file; relative links resolve against the Piff, or the file's
    /// parent, the way they do on GitHub. A link to a Piff's content file
    /// (`child/PIFF.md`) opens the Piff, even if the file does not exist yet.
    pub fn resolve_link(&self, from: &str, href: &str) -> Result<Target> {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            return Ok(Target::Anchor {
                id: href.trim_start_matches('#').to_string(),
            });
        }
        if href.starts_with("//") || has_scheme(href) {
            let lower = href.to_ascii_lowercase();
            return if ["http://", "https://", "mailto:"]
                .iter()
                .any(|s| lower.starts_with(s))
            {
                Ok(Target::External {
                    url: href.to_string(),
                })
            } else {
                Err(format!("NoDir does not open {href} links"))
            };
        }

        let from = self.contain(from)?;
        let base = if from.is_dir() {
            from
        } else {
            from.parent().map(Path::to_path_buf).unwrap_or(from)
        };
        let path_part = href.split(['#', '?']).next().unwrap_or_default();
        let decoded = percent_decode_str(path_part).decode_utf8_lossy();
        let target = match decoded.strip_prefix('/') {
            // Root-relative, as on GitHub, where `/` is the repository root.
            Some(rest) => self.path.join(rest),
            None => base.join(decoded.as_ref()),
        };
        let target = self.contain(target)?;

        if target.is_dir() {
            return Ok(Target::Piff {
                path: display(&target),
            });
        }
        if target.file_name().is_some_and(|n| n == CONTENT_FILE_NAME) {
            if let Some(dir) = target.parent().filter(|d| d.is_dir()) {
                return Ok(Target::Piff { path: display(dir) });
            }
        }
        if target.is_file() {
            return Ok(Target::File {
                path: display(&target),
            });
        }
        Err(format!("{href} does not exist"))
    }
    /// Creates a Piff named `name` in the Piff at `parent`, with an initial
    /// `# title` in its `PIFF.md` if `title` is given.
    pub fn create_piff(&self, parent: &str, name: &str, title: Option<&str>) -> Result<Done> {
        Ok(ops::create_piff(&self.contain(parent)?, name, title)
            .map_err(|e| e.to_string())?
            .into())
    }

    /// Creates an empty file named `name` in the Piff at `parent`.
    pub fn create_file(&self, parent: &str, name: &str) -> Result<Done> {
        Ok(ops::create_file(&self.contain(parent)?, name)
            .map_err(|e| e.to_string())?
            .into())
    }

    /// Renames the Piff or file at `path`, in place.
    pub fn rename(&self, path: &str, new_name: &str) -> Result<Done> {
        Ok(ops::rename(&self.entry(path)?, new_name)
            .map_err(|e| e.to_string())?
            .into())
    }

    /// Moves the Piff or file at `path` to the OS trash.
    pub fn trash(&self, path: &str) -> Result<Done> {
        Ok(ops::trash(&self.entry(path)?)
            .map_err(|e| e.to_string())?
            .into())
    }

    /// `path`, if it is inside the root and not the root itself, which the
    /// app never renames or deletes.
    fn entry(&self, path: &str) -> Result<PathBuf> {
        let path = self.contain(path)?;
        if path == self.path {
            return Err("The open folder itself cannot be renamed or deleted".to_string());
        }
        Ok(path)
    }
}

/// The outcome of a file operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    /// The new or renamed entry, or the trashed entry's old path.
    pub path: String,
    /// The Piff the entry is (or was) in.
    pub parent: String,
    /// Set when the parent's children section could not be updated.
    pub warning: Option<String>,
}

impl From<Changed> for Done {
    fn from(done: Changed) -> Self {
        Self {
            path: display(&done.path),
            parent: display(&done.parent),
            warning: done
                .parent_sync
                .err()
                .map(|e| format!("Could not update the children section: {e}")),
        }
    }
}

/// The root folder as shown to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootInfo {
    pub path: String,
    pub name: String,
}

/// One step of the breadcrumb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Crumb {
    pub name: String,
    pub path: String,
}

/// A child in the sidebar tree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_piff: bool,
}

impl From<Child> for Entry {
    fn from(child: Child) -> Self {
        Self {
            is_piff: child.kind == ChildKind::Piff,
            path: display(&child.path),
            name: child.name,
        }
    }
}

/// A Piff, ready to show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiffView {
    pub path: String,
    pub name: String,
    pub doc: PiffDoc,
    pub breadcrumb: Vec<Crumb>,
    pub warning: Option<String>,
}

/// A plain file, ready to show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub name: String,
    pub breadcrumb: Vec<Crumb>,
    pub content: FileContent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileContent {
    Markdown { html: String },
    Text { text: String },
    Binary { size: u64 },
    TooLarge { size: u64 },
}

/// Where a clicked link leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Piff { path: String },
    File { path: String },
    External { url: String },
    Anchor { id: String },
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn piff_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => display(path),
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

/// A URL scheme such as `https:` or `mailto:`. A Windows drive letter
/// (`C:`) does not count.
fn has_scheme(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':') else {
        return false;
    };
    scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Resolves `.` and `..` without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    None | Some(Component::RootDir | Component::Prefix(_))
                ) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> (tempfile::TempDir, Root) {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("Meeting Notes/2026")).unwrap();
        fs::create_dir_all(base.join("empty")).unwrap();
        fs::write(base.join("PIFF.md"), "# Root\n").unwrap();
        fs::write(
            base.join("Meeting Notes/retro.md"),
            "# Retro\n\n[up](../PIFF.md)",
        )
        .unwrap();
        fs::write(base.join("notes.txt"), "plain text").unwrap();
        fs::write(base.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();
        let root = Root::open(base).unwrap();
        (tmp, root)
    }

    fn p(root: &Root, rel: &str) -> String {
        display(&root.path().join(rel))
    }

    #[test]
    fn contain_rejects_escapes() {
        let (_tmp, root) = tree();
        assert!(root.contain(root.path().join("..")).is_err());
        assert!(root.contain(root.path().join("empty/../..")).is_err());
        assert!(root.contain(root.path().join("empty/..")).is_ok());
    }

    #[test]
    fn opening_a_piff_without_content_writes_nothing() {
        let (_tmp, root) = tree();
        let view = root.open_piff(&p(&root, "Meeting Notes")).unwrap();
        assert!(!view.doc.exists);
        assert!(view.doc.children_html.contains("retro.md"));
        assert!(!root.path().join("Meeting Notes/PIFF.md").exists());
        assert!(!root.path().join("empty/PIFF.md").exists());
    }

    #[test]
    fn opening_a_piff_with_content_regenerates_its_section() {
        let (_tmp, root) = tree();
        let view = root.open_piff(&display(root.path())).unwrap();
        assert!(view.doc.exists);
        let on_disk = fs::read_to_string(root.path().join("PIFF.md")).unwrap();
        assert!(on_disk.starts_with("# Root\n"));
        assert!(on_disk.contains("(Meeting%20Notes/PIFF.md)"), "{on_disk}");
        assert_eq!(view.doc.text, on_disk);
        assert!(view.doc.section.is_some());
    }

    #[test]
    fn breadcrumb_runs_from_root() {
        let (_tmp, root) = tree();
        let crumbs = root.breadcrumb(&root.path().join("Meeting Notes/2026"));
        let names: Vec<_> = crumbs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names[1..], ["Meeting Notes", "2026"]);
        assert_eq!(crumbs[0].path, display(root.path()));
    }

    #[test]
    fn resolves_child_piff_links() {
        let (_tmp, root) = tree();
        let from = display(root.path());
        let want = Target::Piff {
            path: p(&root, "Meeting Notes"),
        };
        // The content file need not exist.
        assert_eq!(
            root.resolve_link(&from, "Meeting%20Notes/PIFF.md").unwrap(),
            want
        );
        assert_eq!(root.resolve_link(&from, "Meeting%20Notes/").unwrap(), want);
    }

    #[test]
    fn resolves_links_from_files_against_their_parent() {
        let (_tmp, root) = tree();
        let from = p(&root, "Meeting Notes/retro.md");
        assert_eq!(
            root.resolve_link(&from, "../PIFF.md").unwrap(),
            Target::Piff {
                path: display(root.path())
            }
        );
        assert_eq!(
            root.resolve_link(&from, "/notes.txt#x").unwrap(),
            Target::File {
                path: p(&root, "notes.txt")
            }
        );
    }

    #[test]
    fn link_kinds() {
        let (_tmp, root) = tree();
        let from = display(root.path());
        assert!(matches!(
            root.resolve_link(&from, "https://example.com").unwrap(),
            Target::External { .. }
        ));
        assert!(matches!(
            root.resolve_link(&from, "#top").unwrap(),
            Target::Anchor { .. }
        ));
        assert!(root.resolve_link(&from, "javascript:alert(1)").is_err());
        assert!(root.resolve_link(&from, "file:///etc/passwd").is_err());
        assert!(root.resolve_link(&from, "../outside.md").is_err());
        assert!(root.resolve_link(&from, "missing.md").is_err());
    }

    #[test]
    fn file_views() {
        let (_tmp, root) = tree();
        let md = root.open_file(&p(&root, "Meeting Notes/retro.md")).unwrap();
        assert!(
            matches!(md.content, FileContent::Markdown { ref html } if html.contains("<h1>Retro</h1>"))
        );
        let txt = root.open_file(&p(&root, "notes.txt")).unwrap();
        assert!(matches!(txt.content, FileContent::Text { ref text } if text == "plain text"));
        let bin = root.open_file(&p(&root, "blob.bin")).unwrap();
        assert!(matches!(bin.content, FileContent::Binary { size: 4 }));
    }

    #[test]
    fn children_come_from_core() {
        let (_tmp, root) = tree();
        let names: Vec<_> = root
            .children(&display(root.path()))
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.is_piff))
            .collect();
        assert_eq!(
            names,
            [
                ("empty".to_string(), true),
                ("Meeting Notes".to_string(), true),
                ("blob.bin".to_string(), false),
                ("notes.txt".to_string(), false),
            ]
        );
    }

    #[test]
    fn file_operations_stay_inside_the_root() {
        let (_tmp, root) = tree();
        let top = display(root.path());
        let done = root.create_piff(&top, "plans", Some("Plans")).unwrap();
        assert_eq!(done.path, p(&root, "plans"));
        assert_eq!(done.parent, top);
        assert!(done.warning.is_none());
        let on_disk = fs::read_to_string(root.path().join("PIFF.md")).unwrap();
        assert!(on_disk.contains("(plans/PIFF.md)"), "{on_disk}");

        root.create_file(&p(&root, "plans"), "a.md").unwrap();
        let renamed = root.rename(&p(&root, "plans"), "Plans 2027").unwrap();
        assert_eq!(renamed.path, p(&root, "Plans 2027"));
        let on_disk = fs::read_to_string(root.path().join("PIFF.md")).unwrap();
        assert!(on_disk.contains("(Plans%202027/PIFF.md)"), "{on_disk}");
        assert!(!on_disk.contains("(plans/PIFF.md)"), "{on_disk}");

        // Nothing outside the root, and never the root itself.
        assert!(root.create_file(&p(&root, ".."), "x").is_err());
        assert!(root.create_file(&top, "../x").is_err());
        assert!(root.rename(&top, "x").is_err());
        assert!(root.trash(&top).is_err());
        assert!(root.trash(&p(&root, "empty/../..")).is_err());
    }

    #[test]
    fn scheme_detection() {
        assert!(has_scheme("https://x"));
        assert!(has_scheme("mailto:a@b"));
        assert!(!has_scheme("C:\\x"));
        assert!(!has_scheme("notes/a.md"));
    }
}
