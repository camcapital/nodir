//! File operations on Piffs and files: create, rename and move to the trash.
//!
//! Each operation changes one entry inside a parent Piff and then regenerates
//! that parent's children section, but only if its `PIFF.md` already exists
//! ([`SyncMode::ExistingOnly`]); an operation never creates a parent's content
//! file. Nothing is ever deleted permanently: [`trash`] moves the entry to the
//! OS trash (Trash, Recycle Bin, or the freedesktop.org trash on Linux).
//!
//! A failed parent sync does not undo the operation, which has already
//! happened on disk. It is reported in [`Changed::parent_sync`] instead, so
//! the caller can warn the user.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::piff::{Piff, SyncMode, SyncOutcome};
use crate::{Error, Result, CONTENT_FILE_NAME};

/// What an operation did.
#[derive(Debug)]
pub struct Changed {
    /// The entry's path after the operation: the new Piff or file, the
    /// renamed entry, or (for [`trash`]) the path it had before it was
    /// trashed.
    pub path: PathBuf,
    /// The parent Piff the entry lives (or lived) in.
    pub parent: PathBuf,
    /// The result of regenerating the parent's children section.
    pub parent_sync: Result<SyncOutcome>,
}

/// Creates the Piff (directory) `name` inside the Piff at `parent`.
///
/// With a `title`, the new Piff gets a `PIFF.md` that starts with
/// `# {title}`, followed by its (empty) children section. Without one, no
/// `PIFF.md` is written: the new Piff is empty. Fails if `name` already
/// exists.
pub fn create_piff(parent: &Path, name: &str, title: Option<&str>) -> Result<Changed> {
    let parent = Piff::open(parent)?;
    let path = parent.dir().join(check_name(name)?);
    fs::create_dir(&path).map_err(|e| Error::io(&path, e))?;
    if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
        // One line only: a newline in the title would start user content
        // the user never typed.
        let title = title.lines().next().unwrap_or_default();
        Piff::open(&path)?.write_content(&format!("# {title}\n"))?;
    }
    Ok(finish(&parent, path))
}

/// Creates the empty file `name` inside the Piff at `parent`. Fails if
/// `name` already exists.
pub fn create_file(parent: &Path, name: &str) -> Result<Changed> {
    let parent = Piff::open(parent)?;
    let path = parent.dir().join(check_name(name)?);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| Error::io(&path, e))?;
    Ok(finish(&parent, path))
}

/// Renames the Piff or file at `path` to `new_name`, in the same parent.
///
/// Fails if another entry named `new_name` already exists; nothing is ever
/// overwritten. A change of case only (`notes` to `Notes`) works on
/// case-insensitive file systems too. A symlink is renamed, not its target.
///
/// The check is made just before renaming, so an entry created by another
/// program in between can still be replaced.
pub fn rename(path: &Path, new_name: &str) -> Result<Changed> {
    let parent = split_entry(path)?;
    let new_name = check_name(new_name)?;
    let new_path = parent.dir().join(new_name);
    if new_path == path {
        return Ok(finish(&parent, new_path));
    }
    if is_taken(path, parent.dir(), new_name)? {
        return Err(Error::io(
            &new_path,
            io::Error::new(io::ErrorKind::AlreadyExists, "already exists"),
        ));
    }
    fs::rename(path, &new_path).map_err(|e| Error::io(path, e))?;
    Ok(finish(&parent, new_path))
}

/// Moves the Piff or file at `path` to the OS trash, from where the user can
/// restore it. A symlink is trashed, not its target.
///
/// On macOS this uses `NSFileManager`, which needs no extra permissions
/// (unlike asking Finder to delete, which prompts for Automation access).
/// Finder's Put Back restores the item.
pub fn trash(path: &Path) -> Result<Changed> {
    remove_with(path, move_to_trash)
}

fn move_to_trash(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let ctx = {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx
    };
    #[cfg(not(target_os = "macos"))]
    let ctx = trash::TrashContext::default();
    ctx.delete(path).map_err(io::Error::other)
}

/// Removes `path` with `remove`, then syncs its parent. Split out from
/// [`trash`] so tests can check everything but the trash itself without
/// filling the developer's Trash.
pub(crate) fn remove_with(
    path: &Path,
    remove: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<Changed> {
    let parent = split_entry(path)?;
    remove(path).map_err(|e| Error::io(path, e))?;
    Ok(finish(&parent, path.to_path_buf()))
}

fn finish(parent: &Piff, path: PathBuf) -> Changed {
    Changed {
        path,
        parent: parent.dir().to_path_buf(),
        parent_sync: parent.sync(SyncMode::ExistingOnly),
    }
}

/// The parent Piff of an existing entry. The content file itself is not an
/// entry: it belongs to its Piff.
fn split_entry(path: &Path) -> Result<Piff> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| invalid(path, "has no name (is it a root?)"))?;
    if name == CONTENT_FILE_NAME {
        return Err(invalid(
            path,
            "is a Piff's content file; rename or delete the Piff instead",
        ));
    }
    fs::symlink_metadata(path).map_err(|e| Error::io(path, e))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| invalid(path, "has no parent Piff"))?;
    Piff::open(parent)
}

/// Checks that `name` is a single, portable file name, and returns it
/// without leading or trailing whitespace, which is easy to type by accident
/// and hard to spot afterwards.
fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    let bad = |why: &str| Err(Error::InvalidName(name.to_string(), why.to_string()));
    if name.is_empty() {
        return bad("a name cannot be empty");
    }
    if name == "." || name == ".." {
        return bad("that name is reserved");
    }
    if name.contains(['/', '\\']) {
        return bad("a name cannot contain / or \\");
    }
    if name.chars().any(char::is_control) {
        return bad("a name cannot contain control characters");
    }
    if name.eq_ignore_ascii_case(CONTENT_FILE_NAME) {
        return bad("PIFF.md holds a Piff's own content");
    }
    if cfg!(windows) {
        if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
            return bad("a name cannot contain < > : \" | ? or *");
        }
        if name.ends_with(['.', ' ']) {
            return bad("a name cannot end with a dot or a space");
        }
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_ascii_uppercase();
        if is_reserved_on_windows(&stem) {
            return bad("that name is reserved on Windows");
        }
    }
    Ok(name)
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9` and `LPT1`-`LPT9`, which
/// Windows reserves whatever the extension.
fn is_reserved_on_windows(stem: &str) -> bool {
    match stem.as_bytes() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" => true,
        [b'C', b'O', b'M', d] | [b'L', b'P', b'T', d] => (b'1'..=b'9').contains(d),
        _ => false,
    }
}

/// Whether renaming `path` to `new_name` in `dir` would replace another
/// entry.
///
/// It would if `dir` has an entry named exactly `new_name`. Otherwise, if
/// `new_name` still resolves to something, the file system matched it
/// case-insensitively, and it is free only if what it resolves to is `path`
/// itself (a case-only rename). That is never assumed from the names alone:
/// on a case-sensitive file system `Notes` and `notes` are two entries.
fn is_taken(path: &Path, dir: &Path, new_name: &str) -> Result<bool> {
    let entries = fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        if entry.file_name() == new_name {
            return Ok(true);
        }
    }
    let new_path = dir.join(new_name);
    // `symlink_metadata` so a dangling symlink still counts as taken.
    match fs::symlink_metadata(&new_path) {
        Ok(_) => Ok(!same_entry(path, &new_path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::io(new_path, e)),
    }
}

/// Whether two paths name the same directory entry, e.g. `notes` and
/// `Notes` on a case-insensitive file system. Compares the entries
/// themselves, never what a symlink points to.
#[cfg(unix)]
fn same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

/// Whether two paths name the same directory entry. Windows has no stable
/// way to read a symlink's own file index, so a symlink only matches a path
/// in the same directory whose name differs in case alone, which on a
/// case-insensitive NTFS directory (the default) is the same entry.
#[cfg(not(unix))]
fn same_entry(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b)) else {
        return false;
    };
    if ma.file_type().is_symlink() || mb.file_type().is_symlink() {
        return ma.file_type().is_symlink() == mb.file_type().is_symlink()
            && a.parent() == b.parent()
            && a.file_name().map(|n| n.to_string_lossy().to_lowercase())
                == b.file_name().map(|n| n.to_string_lossy().to_lowercase());
    }
    same_file::is_same_file(a, b).unwrap_or(false)
}

fn invalid(path: &Path, why: &str) -> Error {
    Error::InvalidName(path.display().to_string(), why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section;
    use tempfile::TempDir;

    /// A root Piff with content, so its children section is kept up to date.
    fn root() -> TempDir {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("PIFF.md"), "# Root\n").unwrap();
        tmp
    }

    fn links(dir: &Path) -> Vec<String> {
        let text = fs::read_to_string(dir.join("PIFF.md")).unwrap();
        section::parse(&text)
            .unwrap()
            .into_iter()
            .map(|l| l.target)
            .collect()
    }

    #[test]
    fn create_piff_updates_the_parent_section() {
        let tmp = root();
        let done = create_piff(tmp.path(), "Meeting Notes", None).unwrap();
        assert!(done.path.is_dir());
        assert_eq!(done.parent_sync.unwrap(), SyncOutcome::Updated);
        assert_eq!(links(tmp.path()), ["Meeting Notes/PIFF.md"]);
        // No title, no content file.
        assert!(!done.path.join("PIFF.md").exists());
        assert!(fs::read_to_string(tmp.path().join("PIFF.md"))
            .unwrap()
            .starts_with("# Root\n"));
    }

    #[test]
    fn create_piff_with_a_title_writes_its_content() {
        let tmp = root();
        let done = create_piff(tmp.path(), "plans", Some("  Plans for 2027\nignored ")).unwrap();
        let text = fs::read_to_string(done.path.join("PIFF.md")).unwrap();
        assert!(text.starts_with("# Plans for 2027\n\n"), "{text}");
        assert_eq!(section::parse(&text).unwrap(), []);
    }

    #[test]
    fn create_never_overwrites() {
        let tmp = root();
        fs::write(tmp.path().join("a.md"), "keep").unwrap();
        assert!(create_file(tmp.path(), "a.md").is_err());
        assert!(create_piff(tmp.path(), "a.md", None).is_err());
        assert_eq!(fs::read_to_string(tmp.path().join("a.md")).unwrap(), "keep");
    }

    #[test]
    fn create_file_updates_the_parent_section() {
        let tmp = root();
        let done = create_file(tmp.path(), "kickoff #1.md").unwrap();
        assert!(done.path.is_file());
        assert_eq!(links(tmp.path()), ["kickoff #1.md"]);
    }

    #[test]
    fn a_parent_without_content_stays_empty() {
        let tmp = TempDir::new().unwrap();
        let done = create_file(tmp.path(), "a.txt").unwrap();
        assert_eq!(done.parent_sync.unwrap(), SyncOutcome::Skipped);
        assert!(!tmp.path().join("PIFF.md").exists());
    }

    #[test]
    fn renaming_a_piff_relinks_it() {
        let tmp = root();
        create_piff(tmp.path(), "old", Some("Old")).unwrap();
        let done = rename(&tmp.path().join("old"), "new name").unwrap();
        assert_eq!(done.path, tmp.path().join("new name"));
        assert!(done.path.join("PIFF.md").is_file());
        assert!(!tmp.path().join("old").exists());
        assert_eq!(links(tmp.path()), ["new name/PIFF.md"]);
    }

    #[test]
    fn rename_never_overwrites() {
        let tmp = root();
        fs::write(tmp.path().join("a"), "a").unwrap();
        fs::write(tmp.path().join("b"), "b").unwrap();
        assert!(rename(&tmp.path().join("a"), "b").is_err());
        assert_eq!(fs::read_to_string(tmp.path().join("b")).unwrap(), "b");
        assert_eq!(fs::read_to_string(tmp.path().join("a")).unwrap(), "a");
    }

    #[test]
    fn rename_can_change_case_only() {
        let tmp = root();
        fs::create_dir(tmp.path().join("notes")).unwrap();
        rename(&tmp.path().join("notes"), "Notes").unwrap();
        let names: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "PIFF.md")
            .collect();
        assert_eq!(names, ["Notes"]);
        assert_eq!(links(tmp.path()), ["Notes/PIFF.md"]);
    }

    #[test]
    fn removing_updates_the_parent_section() {
        let tmp = root();
        create_piff(tmp.path(), "gone", Some("Gone")).unwrap();
        create_file(tmp.path(), "kept.md").unwrap();
        let done = remove_with(&tmp.path().join("gone"), |p| fs::remove_dir_all(p)).unwrap();
        assert_eq!(done.parent, tmp.path());
        assert!(!tmp.path().join("gone").exists());
        assert_eq!(links(tmp.path()), ["kept.md"]);
    }

    #[test]
    fn the_content_file_and_roots_are_not_entries() {
        let tmp = root();
        assert!(rename(&tmp.path().join("PIFF.md"), "x.md").is_err());
        assert!(remove_with(&tmp.path().join("PIFF.md"), |p| fs::remove_file(p)).is_err());
        assert!(tmp.path().join("PIFF.md").exists());
        assert!(remove_with(Path::new("/"), |_| unreachable!()).is_err());
    }

    #[test]
    fn missing_entries_are_errors() {
        let tmp = root();
        assert!(rename(&tmp.path().join("nope"), "x").is_err());
        assert!(remove_with(&tmp.path().join("nope"), |_| unreachable!()).is_err());
    }

    #[test]
    fn names_are_checked() {
        let tmp = root();
        for name in [
            "", "  ", ".", "..", "a/b", "a\\b", "../x", "x\ny", "PIFF.md", "piff.md",
        ] {
            assert!(
                matches!(create_file(tmp.path(), name), Err(Error::InvalidName(..))),
                "{name:?}"
            );
        }
        assert!(matches!(
            rename(&tmp.path().join("PIFF.md"), "x"),
            Err(Error::InvalidName(..))
        ));
        // Hidden names are allowed; they just aren't listed as children.
        create_file(tmp.path(), ".gitignore").unwrap();
        assert_eq!(links(tmp.path()), Vec::<String>::new());
    }

    #[test]
    fn windows_reserved_names() {
        for stem in ["CON", "NUL", "COM1", "LPT9"] {
            assert!(is_reserved_on_windows(stem), "{stem}");
        }
        for stem in ["CONSOLE", "COM0", "COM", "A€", "€A"] {
            assert!(!is_reserved_on_windows(stem), "{stem}");
        }
    }

    #[test]
    fn names_are_trimmed() {
        let tmp = root();
        let done = create_file(tmp.path(), "  notes.md \t").unwrap();
        assert_eq!(done.path, tmp.path().join("notes.md"));
        let done = rename(&done.path, " plan.md ").unwrap();
        assert_eq!(done.path, tmp.path().join("plan.md"));
        assert!(done.path.is_file());
    }

    /// On a case-sensitive file system `Notes` and `notes` are two entries,
    /// even when one of them is a symlink, so renaming one onto the other
    /// must fail rather than replace it.
    #[cfg(target_os = "linux")]
    #[test]
    fn rename_never_overwrites_an_entry_differing_only_in_case() {
        let tmp = root();
        fs::create_dir(tmp.path().join("target")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("target"), tmp.path().join("Notes")).unwrap();
        fs::write(tmp.path().join("notes"), "keep").unwrap();
        assert!(rename(&tmp.path().join("Notes"), "notes").is_err());
        assert!(rename(&tmp.path().join("notes"), "Notes").is_err());
        assert_eq!(
            fs::read_to_string(tmp.path().join("notes")).unwrap(),
            "keep"
        );
        assert!(fs::symlink_metadata(tmp.path().join("Notes"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    /// Where the file system ignores case (macOS and Windows by default), a
    /// symlink's name can change case too.
    #[cfg(unix)]
    #[test]
    fn a_symlink_can_change_case_only() {
        let tmp = root();
        fs::create_dir(tmp.path().join("target")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("target"), tmp.path().join("link")).unwrap();
        if fs::symlink_metadata(tmp.path().join("LINK")).is_err() {
            return; // case-sensitive: `LINK` is simply a new name
        }
        rename(&tmp.path().join("link"), "Link").unwrap();
        let names: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"Link".to_string()), "{names:?}");
        assert!(!names.contains(&"link".to_string()), "{names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn renaming_a_symlink_renames_the_link() {
        let tmp = root();
        fs::create_dir(tmp.path().join("target")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("target"), tmp.path().join("link")).unwrap();
        rename(&tmp.path().join("link"), "renamed").unwrap();
        assert!(tmp.path().join("target").is_dir());
        assert!(fs::symlink_metadata(tmp.path().join("renamed"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    /// Moves a real file to the OS trash. Ignored by default so that running
    /// the tests doesn't fill the developer's Trash; run it with
    /// `cargo test -p nodir-core -- --ignored`.
    #[test]
    #[ignore = "uses the real OS trash"]
    fn trash_moves_to_the_os_trash() {
        let tmp = root();
        create_file(tmp.path(), "nodir-trash-test.txt").unwrap();
        let done = trash(&tmp.path().join("nodir-trash-test.txt")).unwrap();
        assert!(!done.path.exists());
        assert_eq!(links(tmp.path()), Vec::<String>::new());
    }
}
