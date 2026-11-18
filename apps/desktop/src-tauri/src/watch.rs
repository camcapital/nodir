//! Watches the open Piff's content file for changes made outside the app.
//!
//! Only one file is watched at a time, the `PIFF.md` of the Piff being
//! edited; there is no whole-tree watcher. The watch is on the Piff's
//! directory (non-recursive) rather than the file, because many editors
//! save by writing a new file and renaming it over the old one, and because
//! the file may not exist yet.

use std::path::{Path, PathBuf};

use nodir_core::CONTENT_FILE_NAME;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::browse::Result;

/// The event sent to the webview when the watched `PIFF.md` changes.
pub const EVENT: &str = "piff-changed";

/// The current watch. Dropping it stops watching.
pub struct PiffWatch {
    _watcher: RecommendedWatcher,
}

/// Starts watching `dir`'s content file, calling `on_change` (on a
/// background thread) whenever it may have changed.
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<PiffWatch> {
    let real_dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if relevant(&event, &real_dir) {
            on_change();
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Could not watch {}: {e}", dir.display()))?;
    Ok(PiffWatch { _watcher: watcher })
}

/// Whether `event` may have changed `dir`'s content file. Reads (access
/// events) are ignored, or the app's own reads would trigger reloads.
fn relevant(event: &notify::Event, dir: &Path) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|p| is_content_file(p, dir))
}

fn is_content_file(path: &Path, dir: &Path) -> bool {
    if path.file_name().is_none_or(|n| n != CONTENT_FILE_NAME) {
        return false;
    }
    // Some backends report paths with symlinks resolved (/private/var on
    // macOS), so compare canonical parents.
    let parent = path.parent().map(PathBuf::from).unwrap_or_default();
    let parent = dunce::canonicalize(&parent).unwrap_or(parent);
    parent == dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind};
    use notify::Event;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn reports_changes_to_the_content_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let _watch = watch(tmp.path(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        // Give the backend a moment to start (FSEvents on macOS).
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(tmp.path().join("PIFF.md"), "hello").unwrap();
        rx.recv_timeout(Duration::from_secs(10))
            .expect("a change to PIFF.md is reported");
    }

    #[test]
    fn only_the_content_file_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = dunce::canonicalize(tmp.path()).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let event = |kind, path: PathBuf| Event::new(kind).add_path(path);
        let modify = EventKind::Modify(ModifyKind::Any);

        assert!(relevant(&event(modify, dir.join("PIFF.md")), &dir));
        assert!(relevant(
            &event(EventKind::Create(CreateKind::File), dir.join("PIFF.md")),
            &dir
        ));
        assert!(!relevant(&event(modify, dir.join("notes.md")), &dir));
        assert!(!relevant(&event(modify, dir.join("sub/PIFF.md")), &dir));
        assert!(!relevant(
            &event(EventKind::Access(AccessKind::Any), dir.join("PIFF.md")),
            &dir
        ));
    }
}
