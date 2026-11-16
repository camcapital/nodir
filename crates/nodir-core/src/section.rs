//! The children section: the block of `PIFF.md` that NoDir owns.
//!
//! NoDir writes the list of a Piff's children between [`START_MARKER`] and
//! [`END_MARKER`]. Everything outside the markers belongs to the user and is
//! preserved byte-for-byte: [`apply`] only ever replaces the text from the
//! start of the start marker to the end of the end marker, or appends a new
//! block when the markers are missing.

use std::ops::Range;

use crate::children::{Child, ChildKind};

/// Opens the NoDir-owned children section.
pub const START_MARKER: &str = "<!-- nodir:children:start -->";
/// Closes the NoDir-owned children section.
pub const END_MARKER: &str = "<!-- nodir:children:end -->";

/// A Piff's content split around its children section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split<'a> {
    /// User content before the start marker.
    pub before: &'a str,
    /// The section itself, markers included, if the content has one.
    pub section: Option<&'a str>,
    /// User content after the end marker (usually just a line ending).
    pub after: &'a str,
}

impl Split<'_> {
    /// The user-owned content with the children section removed.
    pub fn user_content(&self) -> String {
        [self.before, self.after].concat()
    }
}

/// One link parsed out of a children section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildLink {
    /// The link text, with markdown escapes removed.
    pub text: String,
    /// The link target, percent-decoded. A child Piff's target ends in its
    /// content file name, for example `notes/PIFF.md`.
    pub target: String,
}

/// Locates the children section: the last start marker before the last end
/// marker. Returns the byte range from the start of the start marker to the
/// end of the end marker.
///
/// Taking the *last* pair means a stray marker the user typed earlier in the
/// file (or a start marker left without an end) stays user content, and the
/// block NoDir appended at the end is the one that gets regenerated.
pub fn find(content: &str) -> Option<Range<usize>> {
    let end = content.rfind(END_MARKER)?;
    let start = content[..end].rfind(START_MARKER)?;
    Some(start..end + END_MARKER.len())
}

/// Splits content around its children section.
pub fn split(content: &str) -> Split<'_> {
    match find(content) {
        Some(range) => Split {
            before: &content[..range.start],
            section: Some(&content[range.clone()]),
            after: &content[range.end..],
        },
        None => Split {
            before: content,
            section: None,
            after: "",
        },
    }
}

/// The line ending used by `content`: CRLF if its first line ends in CRLF,
/// otherwise LF (also for content with no line breaks at all).
pub fn line_ending(content: &str) -> &'static str {
    match content.find('\n') {
        Some(i) if content[..i].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Renders the children section, markers included, with no trailing line
/// ending. `content_file` is the file name a child Piff's link points to.
pub fn render(children: &[Child], content_file: &str, newline: &str) -> String {
    let mut out = String::from(START_MARKER);
    out.push_str(newline);
    for child in children {
        let (text, target) = match child.kind {
            ChildKind::Piff => (
                format!("{}/", child.name),
                format!("{}/{}", encode(&child.name), encode(content_file)),
            ),
            ChildKind::File => (child.name.clone(), encode(&child.name)),
        };
        out.push_str("- [");
        out.push_str(&escape_text(&text));
        out.push_str("](");
        out.push_str(&target);
        out.push(')');
        out.push_str(newline);
    }
    out.push_str(END_MARKER);
    out
}

/// Returns `content` with its children section regenerated from `children`.
///
/// Bytes outside the section are never changed. When the markers are missing
/// the section is appended at the end, after a blank line, so the original
/// content is an exact prefix of the result. The section uses the content's
/// own line ending (see [`line_ending`]).
pub fn apply(content: &str, children: &[Child], content_file: &str) -> String {
    let newline = line_ending(content);
    let block = render(children, content_file, newline);
    if let Some(range) = find(content) {
        return [&content[..range.start], &block, &content[range.end..]].concat();
    }
    let separator = if content.is_empty() || content.ends_with(&format!("{newline}{newline}")) {
        String::new()
    } else if content.ends_with('\n') {
        newline.to_string()
    } else {
        format!("{newline}{newline}")
    };
    [content, &separator, &block, newline].concat()
}

/// Parses the links in the children section, or `None` if there is no
/// section. Lines that are not `- [text](target)` links are skipped.
pub fn parse(content: &str) -> Option<Vec<ChildLink>> {
    let range = find(content)?;
    let inner = &content[range.start + START_MARKER.len()..range.end - END_MARKER.len()];
    Some(inner.lines().filter_map(parse_link_line).collect())
}

fn parse_link_line(line: &str) -> Option<ChildLink> {
    let rest = line.trim().strip_prefix("- [")?;
    let mut text = String::new();
    let mut chars = rest.char_indices();
    let close = loop {
        match chars.next()? {
            (_, '\\') => text.push(chars.next()?.1),
            (i, ']') => break i,
            (_, c) => text.push(c),
        }
    };
    let target = rest[close + 1..].strip_prefix('(')?.strip_suffix(')')?;
    Some(ChildLink {
        text,
        target: decode(target),
    })
}

/// Characters that are escaped in link text so a file name can never break
/// out of the link or turn into emphasis, code or HTML.
const ESCAPED: &[char] = &['\\', '[', ']', '*', '_', '`', '<', '>', '&', '~'];

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ESCAPED.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Percent-encodes one path segment, keeping only RFC 3986 unreserved
/// characters. Spaces, parentheses, `#`, `?`, `:` and non-ASCII bytes are all
/// encoded, so the link resolves on GitHub and in any CommonMark renderer.
fn encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode(target: &str) -> String {
    let bytes = target.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FILE: &str = "PIFF.md";

    fn piff(name: &str) -> Child {
        Child {
            name: name.into(),
            path: PathBuf::from(name),
            kind: ChildKind::Piff,
        }
    }

    fn file(name: &str) -> Child {
        Child {
            name: name.into(),
            path: PathBuf::from(name),
            kind: ChildKind::File,
        }
    }

    fn sample() -> Vec<Child> {
        vec![piff("docs"), file("notes.txt")]
    }

    /// Syncs `content` several times with changing children and checks the
    /// user's bytes survive every pass untouched.
    fn assert_round_trip(original: &str) {
        let children_sets = [sample(), vec![file("a.md")], vec![], sample()];
        let mut content = original.to_string();
        for children in &children_sets {
            let next = apply(&content, children, FILE);
            // The original content is always an exact prefix: nothing the
            // user wrote moves or changes.
            assert!(
                next.starts_with(split(original).before),
                "user content changed:\n{original:?}\n=>\n{next:?}"
            );
            let parts = split(&next);
            assert!(parts.section.is_some());
            // Applying the same children again is a no-op.
            assert_eq!(apply(&next, children, FILE), next);
            content = next;
        }
        // After the first sync the user-owned bytes are fixed for good.
        let first = apply(original, &sample(), FILE);
        let last = apply(&content, &sample(), FILE);
        assert_eq!(first, last);
        assert_eq!(split(&first).before, split(&last).before);
        assert_eq!(split(&first).after, split(&last).after);
    }

    #[test]
    fn round_trip_without_markers() {
        assert_round_trip("# Project\n\nSome notes.\n");
    }

    #[test]
    fn round_trip_without_trailing_newline() {
        assert_round_trip("# Project\n\nNo newline at the end");
        let out = apply("No newline", &sample(), FILE);
        assert!(out.starts_with("No newline\n\n<!-- nodir:children:start -->\n"));
    }

    #[test]
    fn round_trip_with_crlf() {
        let original = "# Project\r\n\r\nWindows notes.\r\n";
        assert_round_trip(original);
        let out = apply(original, &sample(), FILE);
        assert!(out.starts_with(original));
        // Every line ending NoDir adds is CRLF too.
        assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
        assert!(out.ends_with("<!-- nodir:children:end -->\r\n"));
    }

    #[test]
    fn round_trip_crlf_without_trailing_newline() {
        let original = "# Project\r\n\r\nlast line";
        assert_round_trip(original);
        let out = apply(original, &sample(), FILE);
        assert!(out.starts_with("# Project\r\n\r\nlast line\r\n\r\n<!-- nodir"));
    }

    #[test]
    fn round_trip_empty_and_blank_line_endings() {
        assert_round_trip("");
        assert_round_trip("\n");
        assert_round_trip("Ends with a blank line\n\n");
        let out = apply("Ends with a blank line\n\n", &[], FILE);
        assert_eq!(
            out,
            "Ends with a blank line\n\n<!-- nodir:children:start -->\n<!-- nodir:children:end -->\n"
        );
    }

    #[test]
    fn existing_section_is_replaced_and_surroundings_kept() {
        let before = "# Title\r\n\r\nIntro.\r\n\r\n";
        let after = "\r\nUser text after the section.\r\n";
        let content = format!("{before}{START_MARKER}\r\n- [old](old)\r\n{END_MARKER}{after}");
        let out = apply(&content, &sample(), FILE);
        assert!(out.starts_with(before));
        assert!(out.ends_with(after));
        assert!(!out.contains("[old]"));
        assert_round_trip(&content);
    }

    #[test]
    fn stray_markers_in_user_content_are_left_alone() {
        // A lone end marker before the real section, and a start marker
        // with no end: both are user text.
        let content = format!(
            "Mentions {END_MARKER} inline.\nAnd {START_MARKER} unterminated.\n\n{START_MARKER}\n{END_MARKER}\n"
        );
        let out = apply(&content, &sample(), FILE);
        assert!(out.starts_with(&format!(
            "Mentions {END_MARKER} inline.\nAnd {START_MARKER} unterminated.\n\n"
        )));
        assert_round_trip(&content);

        let unterminated = format!("Text\n{START_MARKER}\n- [x](x)\n");
        let out = apply(&unterminated, &sample(), FILE);
        assert!(out.starts_with(&unterminated));
        assert_round_trip(&unterminated);
    }

    #[test]
    fn utf8_bom_is_preserved() {
        assert_round_trip("\u{feff}# Title\n");
        assert!(apply("\u{feff}# Title\n", &[], FILE).starts_with("\u{feff}# Title\n"));
    }

    #[test]
    fn renders_piff_and_file_links() {
        let out = apply("", &sample(), FILE);
        assert_eq!(
            out,
            "<!-- nodir:children:start -->\n- [docs/](docs/PIFF.md)\n- [notes.txt](notes.txt)\n<!-- nodir:children:end -->\n"
        );
    }

    #[test]
    fn awkward_names_are_encoded_and_escaped() {
        let children = vec![
            piff("My Notes (old)"),
            file("a_b*[c]`d`.md"),
            file("100% #1?.txt"),
            file("résumé.md"),
            file("c:thing"),
        ];
        let out = apply("", &children, FILE);
        assert!(out.contains("- [My Notes (old)/](My%20Notes%20%28old%29/PIFF.md)"));
        assert!(out.contains(r"- [a\_b\*\[c\]\`d\`.md](a_b%2A%5Bc%5D%60d%60.md)"));
        assert!(out.contains("- [100% #1?.txt](100%25%20%231%3F.txt)"));
        assert!(out.contains("- [résumé.md](r%C3%A9sum%C3%A9.md)"));
        assert!(out.contains("- [c:thing](c%3Athing)"));

        let links = parse(&out).unwrap();
        let texts: Vec<_> = links.iter().map(|l| l.text.as_str()).collect();
        let targets: Vec<_> = links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(
            texts,
            [
                "My Notes (old)/",
                "a_b*[c]`d`.md",
                "100% #1?.txt",
                "résumé.md",
                "c:thing"
            ]
        );
        assert_eq!(
            targets,
            [
                "My Notes (old)/PIFF.md",
                "a_b*[c]`d`.md",
                "100% #1?.txt",
                "résumé.md",
                "c:thing"
            ]
        );
    }

    #[test]
    fn parse_round_trips_render() {
        let out = apply("# Hi\r\n", &sample(), FILE);
        assert_eq!(
            parse(&out).unwrap(),
            vec![
                ChildLink {
                    text: "docs/".into(),
                    target: "docs/PIFF.md".into()
                },
                ChildLink {
                    text: "notes.txt".into(),
                    target: "notes.txt".into()
                },
            ]
        );
        assert_eq!(parse("# No section\n"), None);
    }

    #[test]
    fn user_content_excludes_section() {
        let out = apply("# Hi\n", &sample(), FILE);
        assert_eq!(split(&out).user_content(), "# Hi\n\n\n");
        assert_eq!(split("plain").user_content(), "plain");
    }
}
