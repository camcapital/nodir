//! Renders markdown as ANSI-styled terminal text for `nodir show`.
//!
//! HTML comments (including the children-section markers) are hidden. Lines
//! are not wrapped; the terminal does that.

use std::borrow::Cow;

use nodir_core::section::START_MARKER;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

const BOLD: &str = "1";
const DIM: &str = "2";
const ITALIC: &str = "3";
const UNDERLINE: &str = "4";
const STRIKE: &str = "9";
const BLUE: &str = "34";
const MAGENTA: &str = "35";
const CYAN: &str = "36";
const YELLOW: &str = "33";
const GREEN: &str = "32";

/// Renders `markdown` with ANSI escape codes.
///
/// Control characters in `markdown` (escape sequences, OSC title changes,
/// bells, ...) are removed first, so untrusted content can only ever be
/// styled by NoDir, never drive the terminal itself.
pub fn to_ansi(markdown: &str) -> String {
    let markdown = strip_controls(markdown);
    let mut r = Renderer::default();
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(&markdown, options) {
        r.event(event);
    }
    let mut out = r.out.trim_end().to_string();
    out.push('\n');
    out
}

/// Removes C0 and C1 control characters (including ESC and CR) except tab
/// and line feed, so text is safe to print to a terminal.
pub fn strip_controls(text: &str) -> Cow<'_, str> {
    let keep = |c: char| !c.is_control() || c == '\n' || c == '\t';
    if text.chars().all(keep) {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.chars().filter(|&c| keep(c)).collect())
    }
}

#[derive(Default)]
struct Renderer {
    out: String,
    /// Active SGR codes, innermost last.
    styles: Vec<Vec<&'static str>>,
    /// Line prefixes from enclosing block quotes and list items.
    prefixes: Vec<String>,
    /// Next number for each open list; `None` for bullet lists.
    lists: Vec<Option<u64>>,
    /// Link destinations for open links.
    links: Vec<String>,
    line_start: bool,
    /// Set right after a list marker so the item's first block stays on the
    /// marker's line.
    after_marker: bool,
    in_code_block: bool,
    /// The output ends in a blank line, so no further gap is needed.
    at_gap: bool,
}

impl Renderer {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.push_style(&[YELLOW]);
                self.write(&code);
                self.pop_style();
            }
            Event::Html(html) if html.contains(START_MARKER) => {
                self.block_gap();
                self.push_style(&[DIM]);
                self.write("Children");
                self.pop_style();
                self.newline();
                // Keep the list directly under its label.
                self.after_marker = true;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if !html.trim_start().starts_with("<!--") {
                    self.push_style(&[DIM]);
                    self.text(html.trim_end_matches('\n'));
                    self.pop_style();
                }
            }
            Event::SoftBreak => self.write(" "),
            Event::HardBreak => self.newline(),
            Event::Rule => {
                self.block_gap();
                self.push_style(&[DIM]);
                self.write(&"─".repeat(40));
                self.pop_style();
                self.newline();
            }
            Event::TaskListMarker(done) => self.write(if done { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(name) => self.write(&format!("[^{name}]")),
            Event::InlineMath(m) | Event::DisplayMath(m) => self.write(&m),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.block_gap(),
            Tag::Heading { level, .. } => {
                self.block_gap();
                let codes: &[&str] = match level {
                    HeadingLevel::H1 => &[BOLD, UNDERLINE, MAGENTA],
                    HeadingLevel::H2 => &[BOLD, MAGENTA],
                    _ => &[BOLD, CYAN],
                };
                self.push_style(codes);
            }
            Tag::BlockQuote(_) => {
                self.block_gap();
                self.prefixes.push(style("│ ", &[DIM]));
            }
            Tag::CodeBlock(kind) => {
                self.block_gap();
                if let CodeBlockKind::Fenced(lang) = kind {
                    if !lang.is_empty() {
                        self.push_style(&[DIM]);
                        self.write(&lang);
                        self.pop_style();
                        self.newline();
                    }
                }
                self.in_code_block = true;
                self.prefixes.push("    ".into());
                self.push_style(&[GREEN]);
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block_gap();
                } else if !self.line_start {
                    self.newline();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                if !self.line_start {
                    self.newline();
                }
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.write(&marker);
                self.prefixes.push(" ".repeat(marker.chars().count()));
                self.after_marker = true;
            }
            Tag::Emphasis => self.push_style(&[ITALIC]),
            Tag::Strong => self.push_style(&[BOLD]),
            Tag::Strikethrough => self.push_style(&[STRIKE]),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.links.push(dest_url.into_string());
                self.push_style(&[UNDERLINE, BLUE]);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.newline(),
            TagEnd::Heading(_) => {
                self.pop_style();
                self.newline();
            }
            TagEnd::BlockQuote(_) => {
                self.prefixes.pop();
            }
            TagEnd::CodeBlock => {
                self.pop_style();
                self.prefixes.pop();
                self.in_code_block = false;
                if !self.line_start {
                    self.newline();
                }
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Item => {
                self.prefixes.pop();
                if !self.line_start {
                    self.newline();
                }
                self.after_marker = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link | TagEnd::Image => {
                self.pop_style();
                let dest = self.links.pop().unwrap_or_default();
                if dest.contains("://") {
                    self.push_style(&[DIM]);
                    self.write(&format!(" <{dest}>"));
                    self.pop_style();
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        let text = if self.in_code_block {
            text.strip_suffix('\n').unwrap_or(text)
        } else {
            text
        };
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.newline();
            }
            if !line.is_empty() {
                self.write(line);
            }
        }
    }

    /// Writes inline text, emitting the line prefix first at a line start.
    fn write(&mut self, s: &str) {
        if self.line_start {
            self.out.push_str("\x1b[0m");
            for p in &self.prefixes {
                self.out.push_str(p);
            }
            self.out.push_str(&self.sgr());
            self.line_start = false;
        }
        self.after_marker = false;
        self.at_gap = false;
        self.out.push_str(s);
    }

    fn newline(&mut self) {
        if !self.styles.is_empty() {
            self.out.push_str("\x1b[0m");
        }
        self.out.push('\n');
        self.line_start = true;
    }

    /// Separates blocks with one blank line (carrying any quote prefix).
    fn block_gap(&mut self) {
        if self.out.is_empty() || self.after_marker {
            return;
        }
        if !self.line_start {
            self.newline();
        }
        if !self.at_gap {
            self.out.push_str("\x1b[0m");
            let prefix: String = self.prefixes.concat();
            self.out.push_str(prefix.trim_end());
            self.out.push_str("\x1b[0m");
            self.out.push('\n');
            self.at_gap = true;
        }
    }

    fn sgr(&self) -> String {
        let codes: Vec<&str> = self.styles.iter().flatten().copied().collect();
        if codes.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", codes.join(";"))
        }
    }

    fn push_style(&mut self, codes: &[&'static str]) {
        self.styles.push(codes.to_vec());
        if !self.line_start {
            self.out.push_str(&self.sgr());
        }
    }

    fn pop_style(&mut self) {
        self.styles.pop();
        if !self.line_start {
            self.out.push_str("\x1b[0m");
            self.out.push_str(&self.sgr());
        }
    }
}

fn style(s: &str, codes: &[&str]) -> String {
    format!("\x1b[{}m{s}\x1b[0m", codes.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strips ANSI escape sequences.
    fn plain(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn renders_blocks_and_hides_markers() {
        let md = "# Title\n\nSome *em* and **strong** and `code`.\n\n> quoted\n> more\n\n- one\n- two\n  1. nested\n\n```rust\nfn main() {}\n```\n\n<!-- nodir:children:start -->\n- [docs/](docs/PIFF.md)\n<!-- nodir:children:end -->\n";
        let out = plain(&to_ansi(md));
        assert_eq!(
            out,
            "Title\n\nSome em and strong and code.\n\n│ quoted more\n\n• one\n• two\n  1. nested\n\nrust\n    fn main() {}\n\nChildren\n• docs/\n"
        );
    }

    #[test]
    fn external_links_show_their_target() {
        let out = plain(&to_ansi(
            "[site](https://example.com) and [local](a/PIFF.md)\n",
        ));
        assert_eq!(out, "site <https://example.com> and local\n");
    }

    #[test]
    fn terminal_control_sequences_in_content_are_stripped() {
        let md = "hi \x1b[31mRED\x1b]0;pwned\x07 \u{9b}2J done\r\n\n```\n\x1b[2Jcode\n```\n";
        let out = to_ansi(md);
        // Only NoDir's own SGR sequences remain: ESC followed by `[` and
        // digits/semicolons ending in `m`.
        let mut rest = out.as_str();
        while let Some(i) = rest.find('\x1b') {
            let seq = &rest[i + 1..];
            let end = seq.find('m').expect("unterminated escape");
            assert!(seq.starts_with('['), "{out:?}");
            assert!(
                seq[1..end].chars().all(|c| c.is_ascii_digit() || c == ';'),
                "{out:?}"
            );
            rest = &seq[end..];
        }
        assert!(!out.contains('\x07') && !out.contains('\u{9b}') && !out.contains('\r'));
        assert_eq!(plain(&out), "hi [31mRED]0;pwned 2J done\n\n    [2Jcode\n");
        assert_eq!(strip_controls("tab\tok\n"), "tab\tok\n");
    }

    #[test]
    fn styles_are_reset_at_line_ends() {
        let out = to_ansi("**bold\nline**\n");
        for line in out.lines() {
            assert!(!line.ends_with("\x1b[1m"));
        }
    }
}
