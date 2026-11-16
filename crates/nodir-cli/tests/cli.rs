//! End-to-end tests for the `nodir` subcommands.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use nodir_core::section;
use tempfile::TempDir;

fn nodir() -> Command {
    let mut cmd = Command::cargo_bin("nodir").unwrap();
    cmd.env_remove("VISUAL").env_remove("EDITOR");
    cmd
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "x").unwrap();
}

/// Every file and directory under `root`, relative to it.
fn snapshot(root: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.insert(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    out
}

/// A small project tree with awkward names.
fn sample_tree() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join(".gitignore"), "build/\n").unwrap();
    touch(&root.join("README.md"));
    touch(&root.join("build/out.bin"));
    touch(&root.join(".secret/key"));
    touch(&root.join("docs/guide.md"));
    touch(&root.join("docs/My Notes (2024)/draft #1.md"));
    touch(&root.join("src/core/lib.rs"));
    touch(&root.join("src/résumé.txt"));
    tmp
}

#[test]
fn show_without_piff_md_creates_no_files() {
    let tmp = sample_tree();
    let before = snapshot(tmp.path());
    let out = nodir().arg("show").arg(tmp.path()).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    // Not a TTY: raw markdown with the children section.
    assert!(stdout.starts_with(section::START_MARKER));
    assert!(stdout.contains("- [docs/](docs/PIFF.md)"));
    assert!(stdout.contains("- [README.md](README.md)"));
    assert!(!stdout.contains("build"));
    assert!(!stdout.contains(".secret"));
    assert_eq!(snapshot(tmp.path()), before);
}

#[test]
fn show_prints_existing_content_with_fresh_children() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("PIFF.md"),
        "# Hello\r\n\r\nDO NOT TRUST\r\n",
    )
    .unwrap();
    touch(&tmp.path().join("new.txt"));
    let out = nodir().arg("show").arg(tmp.path()).assert().success();
    assert_eq!(
        String::from_utf8(out.get_output().stdout.clone()).unwrap(),
        "# Hello\r\n\r\nDO NOT TRUST\r\n\r\n<!-- nodir:children:start -->\r\n- [new.txt](new.txt)\r\n<!-- nodir:children:end -->\r\n"
    );
    // show does not write the refreshed section back.
    assert_eq!(
        fs::read_to_string(tmp.path().join("PIFF.md")).unwrap(),
        "# Hello\r\n\r\nDO NOT TRUST\r\n"
    );
}

#[test]
fn ls_lists_piffs_then_files() {
    let tmp = sample_tree();
    nodir()
        .arg("ls")
        .arg(tmp.path().join("docs"))
        .assert()
        .success()
        .stdout("My Notes (2024)/\nguide.md\n");
    nodir()
        .arg("ls")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout("docs/\nsrc/\nREADME.md\n");
}

#[test]
fn sync_recursive_produces_links_that_resolve() {
    let tmp = sample_tree();
    let root = tmp.path();
    nodir().args(["sync", "-r"]).arg(root).assert().success();

    let expected = ["", "docs", "docs/My Notes (2024)", "src", "src/core"];
    for dir in expected {
        assert!(root.join(dir).join("PIFF.md").is_file(), "{dir}");
    }
    assert!(!root.join("build/PIFF.md").exists());
    assert!(!root.join(".secret/PIFF.md").exists());

    // Every link in every PIFF.md points at a path that exists, relative to
    // that PIFF.md, using `/` separators and percent-encoding: the way
    // GitHub resolves relative links in a rendered markdown file.
    let mut checked = 0;
    for dir in expected {
        let content = fs::read_to_string(root.join(dir).join("PIFF.md")).unwrap();
        for link in section::parse(&content).unwrap() {
            assert!(!link.target.contains('\\'), "{}", link.target);
            let target = root.join(dir).join(&link.target);
            assert!(target.exists(), "{dir}: broken link {}", link.target);
            checked += 1;
        }
    }
    assert_eq!(checked, 9);
    let docs = fs::read_to_string(root.join("docs/PIFF.md")).unwrap();
    assert!(docs.contains("- [My Notes (2024)/](My%20Notes%20%282024%29/PIFF.md)"));

    // A second run changes nothing.
    let out = nodir().args(["sync", "-r"]).arg(root).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(
        stdout,
        "5 Piffs synced: 0 created, 0 updated, 5 unchanged\n"
    );
}

#[test]
fn sync_without_recursive_touches_only_one_piff() {
    let tmp = sample_tree();
    nodir().arg("sync").arg(tmp.path()).assert().success();
    assert!(tmp.path().join("PIFF.md").is_file());
    assert!(!tmp.path().join("docs/PIFF.md").exists());
}

#[test]
fn errors_on_files_and_missing_paths() {
    let tmp = sample_tree();
    nodir()
        .arg("show")
        .arg(tmp.path().join("README.md"))
        .assert()
        .failure()
        .stderr(predicates::str::contains("is not a directory"));
    nodir()
        .arg("ls")
        .arg(tmp.path().join("nope"))
        .assert()
        .failure();
}

#[test]
fn edit_creates_piff_md_and_syncs() {
    let tmp = sample_tree();
    let docs = tmp.path().join("docs");
    // A no-op editor: the file is created with a title and the section.
    let noop = if cfg!(windows) { "rem" } else { "true" };
    nodir()
        .arg("edit")
        .arg(&docs)
        .env("EDITOR", noop)
        .assert()
        .success();
    let content = fs::read_to_string(docs.join("PIFF.md")).unwrap();
    assert!(content.starts_with("# docs\n\n<!-- nodir:children:start -->\n"));
    assert!(content.contains("- [guide.md](guide.md)"));
}

#[cfg(unix)]
#[test]
fn edit_syncs_after_the_editor_exits() {
    let tmp = sample_tree();
    let src = tmp.path().join("src");
    fs::write(src.join("PIFF.md"), "Mine\n").unwrap();
    // The "editor" adds a file next to PIFF.md and appends user text after
    // the section; the post-edit sync must list the file and keep the text.
    let editor = r#"sh -c 'touch "$(dirname "$1")/added.md"; printf "tail\n" >> "$1"' --"#;
    nodir()
        .arg("edit")
        .arg(&src)
        .env("EDITOR", editor)
        .assert()
        .success();
    let content = fs::read_to_string(src.join("PIFF.md")).unwrap();
    assert!(content.starts_with("Mine\n\n<!-- nodir:children:start -->\n"));
    assert!(content.contains("- [added.md](added.md)"));
    assert!(content.ends_with("<!-- nodir:children:end -->\ntail\n"));
}
