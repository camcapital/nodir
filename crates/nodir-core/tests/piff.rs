//! Filesystem tests for the Piff model.

use std::fs;
use std::path::Path;

use nodir_core::section::{self, END_MARKER, START_MARKER};
use nodir_core::{ChildKind, Error, Piff, SyncMode, SyncOutcome};
use tempfile::TempDir;

const FIXTURES: &[&str] = &[
    "crlf.md",
    "no-trailing-newline.md",
    "crlf-no-trailing-newline.md",
    "with-section.md",
    "bom-unicode.md",
];

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, "").unwrap();
}

/// The user-owned bytes of `content`: everything outside the section.
fn user_bytes(content: &str) -> (String, String) {
    let split = section::split(content);
    (split.before.to_string(), split.after.to_string())
}

#[test]
fn fixtures_survive_repeated_syncs_byte_for_byte() {
    for name in FIXTURES {
        let original = fixture(name);
        let original_text = String::from_utf8(original.clone()).unwrap();
        let tmp = TempDir::new().unwrap();
        let piff = Piff::open(tmp.path()).unwrap();
        fs::write(piff.content_path(), &original).unwrap();

        // First sync: with markers missing, the original bytes stay an exact
        // prefix; with markers present, everything outside them is kept.
        touch(&tmp.path().join("b.txt"));
        fs::create_dir(tmp.path().join("a")).unwrap();
        assert_eq!(
            piff.sync(SyncMode::ExistingOnly).unwrap(),
            SyncOutcome::Updated
        );
        let first = piff.read().unwrap().unwrap();
        let (before, after) = user_bytes(&first);
        match section::find(&original_text) {
            None => assert!(first.as_bytes().starts_with(&original), "{name}"),
            Some(range) => {
                assert_eq!(before, original_text[..range.start], "{name}");
                assert_eq!(after, original_text[range.end..], "{name}");
            }
        }

        // Further syncs as children come and go never move a user byte.
        for step in 0..4 {
            match step {
                0 => touch(&tmp.path().join("c d.md")),
                1 => fs::remove_file(tmp.path().join("b.txt")).unwrap(),
                2 => fs::create_dir(tmp.path().join("z")).unwrap(),
                _ => {}
            }
            piff.sync(SyncMode::ExistingOnly).unwrap();
            let now = piff.read().unwrap().unwrap();
            assert_eq!(user_bytes(&now), (before.clone(), after.clone()), "{name}");
        }
        assert_eq!(
            piff.sync(SyncMode::ExistingOnly).unwrap(),
            SyncOutcome::Unchanged
        );

        // CRLF files get CRLF sections.
        let now = piff.read().unwrap().unwrap();
        if original_text.contains("\r\n") {
            assert_eq!(
                now.matches('\n').count(),
                now.matches("\r\n").count(),
                "{name}"
            );
        } else {
            assert!(!now.contains('\r'), "{name}");
        }
    }
}

#[test]
fn reading_never_creates_the_content_file() {
    let tmp = TempDir::new().unwrap();
    touch(&tmp.path().join("file.txt"));
    fs::create_dir(tmp.path().join("child")).unwrap();
    let piff = Piff::open(tmp.path()).unwrap();

    assert!(!piff.has_content());
    assert_eq!(piff.read().unwrap(), None);
    let rendered = piff.render().unwrap();
    assert!(rendered.contains("- [child/](child/PIFF.md)"));
    assert!(rendered.contains("- [file.txt](file.txt)"));
    assert_eq!(piff.children().unwrap().len(), 2);
    assert_eq!(
        piff.sync(SyncMode::ExistingOnly).unwrap(),
        SyncOutcome::Skipped
    );
    assert!(piff
        .sync_recursive(SyncMode::ExistingOnly)
        .iter()
        .all(|r| matches!(r.result, Ok(SyncOutcome::Skipped))));

    assert!(!piff.content_path().exists());
    assert!(!tmp.path().join("child/PIFF.md").exists());
}

#[test]
fn explicit_sync_creates_the_content_file() {
    let tmp = TempDir::new().unwrap();
    touch(&tmp.path().join("file.txt"));
    let piff = Piff::open(tmp.path()).unwrap();
    assert_eq!(
        piff.sync(SyncMode::CreateIfMissing).unwrap(),
        SyncOutcome::Created
    );
    assert_eq!(
        piff.read().unwrap().unwrap(),
        format!("{START_MARKER}\n- [file.txt](file.txt)\n{END_MARKER}\n")
    );
    assert_eq!(
        piff.sync(SyncMode::CreateIfMissing).unwrap(),
        SyncOutcome::Unchanged
    );
}

#[test]
fn children_exclude_hidden_gitignored_and_content_file() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
    touch(&root.join(".hidden"));
    touch(&root.join("PIFF.md"));
    touch(&root.join("debug.log"));
    touch(&root.join("target/out"));
    touch(&root.join("src/main.rs"));
    touch(&root.join("Docs/readme.md"));
    touch(&root.join("b.txt"));
    touch(&root.join("A.txt"));

    let piff = Piff::open(root).unwrap();
    let children = piff.children().unwrap();
    let names: Vec<_> = children.iter().map(|c| (c.name.as_str(), c.kind)).collect();
    assert_eq!(
        names,
        [
            ("Docs", ChildKind::Piff),
            ("src", ChildKind::Piff),
            ("A.txt", ChildKind::File),
            ("b.txt", ChildKind::File),
        ]
    );

    // A parent's .gitignore applies to nested Piffs too.
    touch(&root.join("src/trace.log"));
    let src = Piff::open(root.join("src")).unwrap();
    let names: Vec<_> = src
        .children()
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["main.rs"]);
}

#[test]
fn recursive_sync_skips_ignored_and_hidden_directories() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    touch(&root.join("node_modules/pkg/index.js"));
    touch(&root.join(".cache/x"));
    touch(&root.join("a/b/c/deep.txt"));

    let reports = Piff::open(root)
        .unwrap()
        .sync_recursive(SyncMode::CreateIfMissing);
    assert_eq!(reports.len(), 4); // root, a, a/b, a/b/c
    assert!(reports
        .iter()
        .all(|r| matches!(r.result, Ok(SyncOutcome::Created))));
    assert!(root.join("a/b/c/PIFF.md").is_file());
    assert!(!root.join("node_modules/PIFF.md").exists());
    assert!(!root.join(".cache/PIFF.md").exists());

    // Second pass: nothing to do.
    let reports = Piff::open(root)
        .unwrap()
        .sync_recursive(SyncMode::CreateIfMissing);
    assert!(reports
        .iter()
        .all(|r| matches!(r.result, Ok(SyncOutcome::Unchanged))));
}

#[test]
fn write_content_keeps_section_current() {
    let tmp = TempDir::new().unwrap();
    touch(&tmp.path().join("x.md"));
    let piff = Piff::open(tmp.path()).unwrap();
    assert_eq!(
        piff.write_content("# Hello\n").unwrap(),
        SyncOutcome::Created
    );
    let text = piff.read().unwrap().unwrap();
    assert!(text.starts_with("# Hello\n\n"));
    assert_eq!(section::parse(&text).unwrap()[0].target, "x.md");
    assert_eq!(piff.write_content(&text).unwrap(), SyncOutcome::Unchanged);
}

#[test]
fn non_directories_and_bad_utf8_are_errors() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("plain.txt");
    touch(&file);
    assert!(matches!(Piff::open(&file), Err(Error::NotADirectory(_))));

    fs::write(tmp.path().join("PIFF.md"), b"\xff\xfe bad").unwrap();
    let piff = Piff::open(tmp.path()).unwrap();
    assert!(matches!(
        piff.sync(SyncMode::ExistingOnly),
        Err(Error::NotUtf8(_))
    ));
    assert_eq!(fs::read(piff.content_path()).unwrap(), b"\xff\xfe bad");
}

#[test]
fn parent_gitignore_applies_only_inside_a_repo() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    // No repository: a .gitignore above the opened directory (think
    // ~/.gitignore) must not hide anything.
    fs::write(root.join(".gitignore"), "secret*\n").unwrap();
    touch(&root.join("h/secrets.txt"));
    touch(&root.join("h/secret-dir/x"));
    touch(&root.join("h/plain.txt"));
    let names = |dir: &Path| -> Vec<String> {
        Piff::open(dir)
            .unwrap()
            .children()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect()
    };
    assert_eq!(
        names(&root.join("h")),
        ["secret-dir", "plain.txt", "secrets.txt"]
    );
    // The opened directory's own .gitignore still applies.
    assert_eq!(names(root), ["h"]);
    fs::write(root.join("h/.gitignore"), "*.txt\n!plain.txt\n").unwrap();
    assert_eq!(names(&root.join("h")), ["secret-dir", "plain.txt"]);

    // Inside a repository, .gitignore files up to the repo root apply, and
    // nothing above it.
    fs::create_dir_all(root.join("h/.git")).unwrap();
    touch(&root.join("h/sub/secrets.md"));
    touch(&root.join("h/sub/notes.txt"));
    touch(&root.join("h/sub/plain.txt"));
    assert_eq!(names(&root.join("h/sub")), ["plain.txt", "secrets.md"]);
}

#[test]
fn an_ignored_directory_opened_directly_lists_its_contents() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "build/\n").unwrap();
    touch(&root.join("build/out.bin"));
    let build = Piff::open(root.join("build")).unwrap();
    let names: Vec<_> = build
        .children()
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["out.bin"]);
}

#[test]
fn recursive_sync_continues_past_errors() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(&root.join("a/x.txt"));
    touch(&root.join("b/y.txt"));
    touch(&root.join("c/z.txt"));
    fs::write(root.join("b/PIFF.md"), b"\xff bad").unwrap();

    let reports = Piff::open(root)
        .unwrap()
        .sync_recursive(SyncMode::CreateIfMissing);
    assert_eq!(reports.len(), 4);
    let failed: Vec<_> = reports.iter().filter(|r| r.result.is_err()).collect();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].path.ends_with("b/PIFF.md"));
    assert!(matches!(failed[0].result, Err(Error::NotUtf8(_))));
    // The Piffs after the bad one were still synced, and it was left alone.
    assert!(root.join("c/PIFF.md").is_file());
    assert_eq!(fs::read(root.join("b/PIFF.md")).unwrap(), b"\xff bad");
}

#[test]
fn writes_leave_no_temp_files_and_keep_permissions() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("PIFF.md"), "# Hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("PIFF.md"), fs::Permissions::from_mode(0o640)).unwrap();
    }
    touch(&root.join("a.txt"));
    let piff = Piff::open(root).unwrap();
    assert!(matches!(
        piff.sync(SyncMode::ExistingOnly),
        Ok(SyncOutcome::Updated)
    ));
    let mut entries: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    entries.sort();
    assert_eq!(entries, ["PIFF.md", "a.txt"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join("PIFF.md"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_content_file_is_written_through() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("p")).unwrap();
    fs::write(root.join("real.md"), "# Real\n").unwrap();
    std::os::unix::fs::symlink(root.join("real.md"), root.join("p/PIFF.md")).unwrap();
    touch(&root.join("p/a.txt"));
    Piff::open(root.join("p"))
        .unwrap()
        .sync(SyncMode::ExistingOnly)
        .unwrap();
    assert!(fs::symlink_metadata(root.join("p/PIFF.md"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::read_to_string(root.join("real.md"))
        .unwrap()
        .contains("- [a.txt](a.txt)"));
}

#[test]
fn parent_and_open_child_walk_the_tree() {
    let tmp = TempDir::new().unwrap();
    touch(&tmp.path().join("a/b/file.md"));
    let root = Piff::open(tmp.path()).unwrap();

    let a = root.children().unwrap().remove(0);
    let a = root.open_child(&a).unwrap();
    assert_eq!(a.dir(), tmp.path().join("a"));
    let b = a.open_child(&a.children().unwrap()[0]).unwrap();
    let file = &b.children().unwrap()[0];
    assert_eq!(file.kind, ChildKind::File);
    assert!(matches!(b.open_child(file), Err(Error::NotADirectory(_))));

    let up = b.parent().unwrap().parent().unwrap();
    assert_eq!(up.dir(), tmp.path());
    // Walking up never writes anything.
    assert!(!up.has_content() && !a.has_content() && !b.has_content());

    // Lexical: a bare relative path has no parent to walk to.
    assert!(Piff::open(".").unwrap().parent().is_none());
}
