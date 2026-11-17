//! Markdown to sanitized HTML for the main pane.
//!
//! Content comes from arbitrary files on disk, so the HTML is run through
//! `ammonia` before it reaches the webview: no scripts, no event handlers,
//! no `javascript:` links. Links keep their `href` as written; the frontend
//! intercepts clicks and asks the backend to resolve them.

use std::sync::LazyLock;

use nodir_core::section;
use pulldown_cmark::{html, Options, Parser};

static SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut builder = ammonia::Builder::default();
    // Task-list checkboxes, rendered read-only.
    builder
        .add_tags(["input"])
        .add_tag_attributes("input", ["type", "checked", "disabled"]);
    builder
});

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

/// Renders markdown to sanitized HTML.
pub fn markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3 / 2);
    html::push_html(&mut out, Parser::new_ext(text, options()));
    SANITIZER.clean(&out).to_string()
}

/// Renders a Piff's content. The NoDir-owned children section is wrapped in
/// `<nav class="piff-children">` so it can be styled apart from the user's
/// own content; everything else renders as ordinary markdown.
pub fn piff(content: &str) -> String {
    let split = section::split(content);
    match split.section {
        Some(children) => format!(
            "{}<nav class=\"piff-children\">{}</nav>{}",
            markdown(split.before),
            markdown(children),
            markdown(split.after)
        ),
        None => markdown(content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_handlers() {
        let html = markdown("<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>");
        assert!(!html.contains("<script"));
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn drops_javascript_links() {
        let html = markdown("[x](javascript:alert(1))");
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn keeps_relative_links() {
        let html = markdown("[notes](Meeting%20Notes/PIFF.md)");
        assert!(html.contains("href=\"Meeting%20Notes/PIFF.md\""));
    }

    #[test]
    fn renders_task_lists() {
        let html = markdown("- [x] done\n- [ ] todo");
        assert!(html.contains("checked"));
        assert!(html.contains("disabled"));
    }

    #[test]
    fn wraps_children_section() {
        let content = "# Hi\n\n<!-- nodir:children:start -->\n- [a/](a/PIFF.md)\n<!-- nodir:children:end -->\n";
        let html = piff(content);
        assert!(html.starts_with("<h1>Hi</h1>"));
        assert!(html.contains("<nav class=\"piff-children\">"));
        assert!(html.contains("href=\"a/PIFF.md\""));
    }
}
