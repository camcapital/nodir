//! The interactive Piff browser: `nodir [path]`.
//!
//! A children list on the left, the Piff's rendered markdown on the right.
//! All Piff behaviour comes from `nodir-core`; this module only draws it and
//! maps keys to it.

use std::io::{self, IsTerminal};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};
use nodir_core::{section, Child, Piff, SyncMode};
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::crossterm::{cursor, execute};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame, Terminal};

use crate::editor;
use crate::markdown::{strip_controls, to_text};

const HINTS: &str = "↑↓ select  ⏎/→ open  ←/⌫ parent  PgUp/PgDn scroll  e edit  r reload  q quit";

/// Opens the browser on the Piff at `path` and runs it until the user quits.
pub fn run(path: &Path) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("the browser needs a terminal; use `nodir show` or `nodir ls` in scripts");
    }
    // Open before taking over the screen, so a bad path is a plain error.
    let mut app = App::open(path)?;
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| app.draw(frame))?;
        // Anything but a key (a resize, focus change, ...) just redraws.
        let Event::Key(key) = event::read()? else {
            continue;
        };
        // Windows reports releases too; act once per press.
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match app.handle_key(key) {
            Action::None => {}
            Action::Quit => return Ok(()),
            Action::Edit => suspended(terminal, || app.edit_with(editor::run_editor))?,
            Action::OpenFile(file) => {
                suspended(terminal, || app.open_file_with(&file, editor::run_editor))?
            }
        }
    }
}

/// Hands the terminal back to the user's shell state while `f` runs an
/// external program, then takes it over again.
fn suspended(terminal: &mut DefaultTerminal, f: impl FnOnce()) -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;
    f();
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, Clear(ClearType::All))?;
    // A new terminal has empty buffers, so the next draw repaints everything.
    // (`Terminal::clear` would ask the terminal for the cursor position, which
    // stalls on terminals that don't answer.)
    *terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    Ok(())
}

/// What the event loop should do after a key press.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    /// Edit the current Piff's content in the external editor.
    Edit,
    /// Open an ordinary file in the external editor.
    OpenFile(PathBuf),
}

/// A one-line message in the footer, shown until the next key press.
struct Status {
    text: String,
    error: bool,
}

/// The browser's state.
pub struct App {
    /// Where the browser was opened; the breadcrumb is relative to it.
    root: PathBuf,
    piff: Piff,
    children: Vec<Child>,
    list: ListState,
    content: Text<'static>,
    scroll: u16,
    /// Rows of the content pane at the last draw, for paging.
    page: u16,
    /// The furthest the content can scroll, as of the last draw.
    max_scroll: u16,
    status: Option<Status>,
}

impl App {
    /// Opens the browser state on the Piff at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let dir = normalize(&std::path::absolute(path)?);
        let piff = Piff::open(&dir)?;
        let mut app = Self {
            root: dir,
            piff,
            children: Vec::new(),
            list: ListState::default(),
            content: Text::default(),
            scroll: 0,
            page: 0,
            max_scroll: u16::MAX,
            status: None,
        };
        app.load(None);
        Ok(app)
    }

    /// The Piff being shown.
    #[cfg(test)]
    pub fn piff(&self) -> &Piff {
        &self.piff
    }

    /// The selected child, if any.
    fn selected(&self) -> Option<&Child> {
        self.list.selected().and_then(|i| self.children.get(i))
    }

    /// Reads the current Piff again: syncs its children section if it has a
    /// `PIFF.md` (never creating one), lists its children, and renders its
    /// content. `select` picks the child to highlight; otherwise the current
    /// selection is kept by path.
    fn load(&mut self, select: Option<&Path>) {
        let keep = select
            .map(Path::to_path_buf)
            .or_else(|| self.selected().map(|c| c.path.clone()));

        // Opening a Piff regenerates its section, as everywhere in NoDir.
        if let Err(err) = self.piff.sync(SyncMode::ExistingOnly) {
            self.error(err);
        }
        self.children = match self.piff.children() {
            Ok(children) => children,
            Err(err) => {
                self.error(err);
                Vec::new()
            }
        };
        let index = keep
            .and_then(|path| self.children.iter().position(|c| c.path == path))
            .or_else(|| {
                let old = self.list.selected().unwrap_or(0);
                (!self.children.is_empty()).then(|| old.min(self.children.len() - 1))
            });
        self.list.select(index);

        self.content = match self.piff.read() {
            Ok(Some(text)) => {
                let user = section::split(&text).user_content();
                if user.trim().is_empty() {
                    Text::from(Line::from("No content yet. Press e to write some.").dim())
                } else {
                    to_text(&user)
                }
            }
            Ok(None) => Text::from(vec![
                Line::from(format!("Empty Piff (no {})", self.piff.content_file_name())).dim(),
                Line::default(),
                Line::from("Press e to write one.").dim(),
            ]),
            Err(err) => Text::from(Line::from(strip_controls(&err.to_string()).into_owned()))
                .style(Style::new().fg(Color::Red)),
        };
    }

    fn go_to(&mut self, piff: Piff, select: Option<&Path>) {
        self.piff = piff;
        self.scroll = 0;
        self.max_scroll = u16::MAX;
        self.list.select(None);
        self.load(select);
    }

    fn error(&mut self, err: impl std::fmt::Display) {
        self.status = Some(Status {
            text: strip_controls(&err.to_string()).into_owned(),
            error: true,
        });
    }

    fn info(&mut self, text: String) {
        self.status = Some(Status { text, error: false });
    }

    /// Maps a key press to a change of state, or to an action that needs the
    /// terminal (quitting, running the editor).
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        self.status = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => return Action::Quit,
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Char('d') if ctrl => self.scroll_by(i32::from(self.page / 2).max(1)),
            KeyCode::Char('u') if ctrl => self.scroll_by(-i32::from(self.page / 2).max(1)),
            KeyCode::Down | KeyCode::Char('j') => self.select_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.select_by(-1),
            KeyCode::Home | KeyCode::Char('g') => self.select_by(i32::MIN),
            KeyCode::End | KeyCode::Char('G') => self.select_by(i32::MAX),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(i32::from(self.page).max(1)),
            KeyCode::PageUp | KeyCode::Char('b') => self.scroll_by(-i32::from(self.page).max(1)),
            KeyCode::Char('J') => self.scroll_by(1),
            KeyCode::Char('K') => self.scroll_by(-1),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => return self.open_selected(),
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => self.go_up(),
            KeyCode::Char('e') => return Action::Edit,
            KeyCode::Char('r') => {
                self.load(None);
                if self.status.is_none() {
                    self.info("Reloaded".into());
                }
            }
            _ => {}
        }
        Action::None
    }

    fn select_by(&mut self, delta: i32) {
        if self.children.is_empty() {
            return;
        }
        let last = self.children.len() as i64 - 1;
        let current = self.list.selected().unwrap_or(0) as i64;
        let next = (current + i64::from(delta)).clamp(0, last);
        self.list.select(Some(next as usize));
    }

    fn scroll_by(&mut self, delta: i32) {
        let next = (i32::from(self.scroll) + delta).clamp(0, i32::from(self.max_scroll));
        self.scroll = next as u16;
    }

    fn open_selected(&mut self) -> Action {
        let Some(child) = self.selected().cloned() else {
            return Action::None;
        };
        if !child.is_piff() {
            return Action::OpenFile(child.path);
        }
        match self.piff.open_child(&child) {
            Ok(piff) => self.go_to(piff, None),
            Err(err) => {
                // Most likely removed behind our back; show what is there now.
                self.load(None);
                self.error(err);
            }
        }
        Action::None
    }

    fn go_up(&mut self) {
        let Some(parent) = self.piff.parent() else {
            return;
        };
        let came_from = self.piff.dir().to_path_buf();
        self.go_to(parent, Some(&came_from));
    }

    /// Edits the current Piff with `run_editor` (see [`editor::edit_piff`]),
    /// then reloads it.
    pub fn edit_with(&mut self, run_editor: impl FnOnce(&Path) -> Result<()>) {
        let result = editor::edit_piff(&self.piff, run_editor);
        self.load(None);
        match result {
            Ok(()) => self.info(format!("Saved {}", self.piff.content_file_name())),
            Err(err) => self.error(format!("{err:#}")),
        }
    }

    /// Opens an ordinary file with `run_editor`, then reloads the Piff.
    pub fn open_file_with(&mut self, file: &Path, run_editor: impl FnOnce(&Path) -> Result<()>) {
        let result = run_editor(file);
        self.load(None);
        if let Err(err) = result {
            self.error(format!("{err:#}"));
        }
    }

    /// Draws the whole screen.
    pub fn draw(&mut self, frame: &mut Frame) {
        let [title, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                .areas(body);

        self.draw_title(frame, title);
        self.draw_children(frame, left);
        self.draw_content(frame, right);

        let footer_line = match &self.status {
            Some(Status { text, error: true }) => Line::from(text.as_str()).fg(Color::Red),
            Some(Status { text, error: false }) => Line::from(text.as_str()),
            None => Line::from(HINTS).dim(),
        };
        frame.render_widget(Paragraph::new(footer_line), footer);
    }

    fn draw_title(&self, frame: &mut Frame, area: Rect) {
        let crumbs = self.breadcrumb();
        // Keep the end of a long path, where the current Piff is.
        let room = usize::from(area.width).saturating_sub(2);
        let count = crumbs.chars().count();
        let crumbs = if count > room {
            let tail: String = crumbs.chars().skip(count - room + 1).collect();
            format!("…{tail}")
        } else {
            crumbs
        };
        let line = Line::from(vec![Span::raw(" "), Span::raw(crumbs).bold()]);
        frame.render_widget(Paragraph::new(line).reversed(), area);
    }

    /// The path from where the browser was opened to the current Piff, or the
    /// full path once the user has gone above it.
    fn breadcrumb(&self) -> String {
        let dir = self.piff.dir();
        let text = match dir.strip_prefix(&self.root) {
            Ok(rel) => {
                let root = Piff::open(&self.root)
                    .map(|p| p.name())
                    .unwrap_or_else(|_| self.root.display().to_string());
                std::iter::once(root)
                    .chain(rel.iter().map(|c| c.to_string_lossy().into_owned()))
                    .collect::<Vec<_>>()
                    .join(" / ")
            }
            Err(_) => dir.display().to_string(),
        };
        strip_controls(&text).into_owned()
    }

    fn draw_children(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::new()
            .borders(Borders::RIGHT)
            .title(format!(" Children ({}) ", self.children.len()).bold());
        if self.children.is_empty() {
            let empty = Paragraph::new(Line::from(" No children").dim()).block(block);
            frame.render_widget(empty, area);
            return;
        }
        let items: Vec<ListItem> = self
            .children
            .iter()
            .map(|child| {
                let name = strip_controls(&child.name).into_owned();
                if child.is_piff() {
                    ListItem::new(Line::from(format!("{name}/")).fg(Color::Blue).bold())
                } else {
                    ListItem::new(Line::from(name))
                }
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_symbol("> ")
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_content(&mut self, frame: &mut Frame, area: Rect) {
        let title = if self.piff.has_content() {
            format!(" {} ", self.piff.content_file_name())
        } else {
            " Empty Piff ".into()
        };
        let block = Block::new().title(title.bold()).borders(Borders::NONE);
        let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
        let paragraph = Paragraph::new(self.content.clone()).wrap(Wrap { trim: false });

        let lines = paragraph.line_count(inner.width);
        self.page = inner.height;
        self.max_scroll =
            u16::try_from(lines.saturating_sub(usize::from(inner.height))).unwrap_or(u16::MAX);
        self.scroll = self.scroll.min(self.max_scroll);

        frame.render_widget(block, area);
        frame.render_widget(paragraph.scroll((self.scroll, 0)), inner);
    }
}

/// Resolves `.` and `..` lexically, like a shell's `cd`, so "go to parent"
/// walks the path the user typed rather than a symlink's target.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nodir_core::section::{END_MARKER, START_MARKER};
    use ratatui::backend::TestBackend;
    use tempfile::TempDir;

    use super::*;

    fn press(app: &mut App, code: KeyCode) -> Action {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn screen(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn mkdirs(root: &Path, dirs: &[&str]) {
        for dir in dirs {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
    }

    #[test]
    fn keyboard_navigates_down_several_levels_and_back_to_the_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        mkdirs(root, &["alpha/beta/gamma/delta", "zeta"]);
        fs::write(root.join("alpha/beta/notes.md"), "x").unwrap();
        fs::write(
            root.join("alpha/beta/PIFF.md"),
            "# Beta\n\nThe *beta* Piff.\n",
        )
        .unwrap();

        let mut app = App::open(root).unwrap();
        assert!(screen(&mut app).contains("> alpha/"));

        press(&mut app, KeyCode::Enter); // alpha
        press(&mut app, KeyCode::Char('l')); // beta
        let shown = screen(&mut app);
        assert!(shown.contains("alpha / beta"), "{shown}");
        assert!(shown.contains("The beta Piff."), "{shown}");
        assert!(shown.contains("notes.md"), "{shown}");
        // Opening beta regenerated the section of its existing PIFF.md...
        let beta = fs::read_to_string(root.join("alpha/beta/PIFF.md")).unwrap();
        assert!(beta.contains("- [gamma/](gamma/PIFF.md)"), "{beta}");
        // ...but the section is not repeated in the content pane.
        assert!(!shown.contains(START_MARKER) && !shown.contains("gamma/PIFF.md"));

        press(&mut app, KeyCode::Right); // gamma
        press(&mut app, KeyCode::Enter); // delta
        assert_eq!(app.piff().dir(), root.join("alpha/beta/gamma/delta"));
        assert!(screen(&mut app).contains("No children"));

        for _ in 0..4 {
            press(&mut app, KeyCode::Left);
        }
        assert_eq!(app.piff().dir(), root);
        // The Piff we came back from is selected.
        assert_eq!(app.selected().unwrap().name, "alpha");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected().unwrap().name, "zeta");
        assert_eq!(press(&mut app, KeyCode::Char('q')), Action::Quit);

        // Browsing created no PIFF.md where there was none.
        for dir in [
            "",
            "alpha",
            "alpha/beta/gamma",
            "alpha/beta/gamma/delta",
            "zeta",
        ] {
            assert!(!root.join(dir).join("PIFF.md").exists(), "{dir}");
        }
    }

    #[test]
    fn going_above_the_start_shows_full_paths() {
        let tmp = TempDir::new().unwrap();
        mkdirs(tmp.path(), &["inner"]);
        let mut app = App::open(&tmp.path().join("inner")).unwrap();
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.piff().dir(), tmp.path());
        assert_eq!(app.selected().unwrap().name, "inner");
        assert_eq!(app.breadcrumb(), tmp.path().display().to_string());
    }

    #[test]
    fn edit_round_trips_content_and_regenerates_the_section() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        mkdirs(root, &["docs"]);
        let mut app = App::open(root).unwrap();
        assert!(screen(&mut app).contains("Empty Piff"));
        assert_eq!(press(&mut app, KeyCode::Char('e')), Action::Edit);

        // The "editor" sees the seeded file, rewrites it without a children
        // section, and adds a child Piff meanwhile.
        app.edit_with(|path| {
            let seeded = fs::read_to_string(path).unwrap();
            assert!(seeded.contains("- [docs/](docs/PIFF.md)"), "{seeded}");
            fs::create_dir(root.join("new-piff")).unwrap();
            fs::write(path, "# Hello\n\nNew **content** here.\n").unwrap();
            Ok(())
        });

        let file = fs::read_to_string(root.join("PIFF.md")).unwrap();
        assert!(
            file.starts_with("# Hello\n\nNew **content** here.\n"),
            "{file}"
        );
        assert!(
            file.contains(&format!(
                "{START_MARKER}\n- [docs/](docs/PIFF.md)\n- [new-piff/](new-piff/PIFF.md)\n{END_MARKER}"
            )),
            "{file}"
        );
        let shown = screen(&mut app);
        assert!(shown.contains("New content here."), "{shown}");
        assert!(shown.contains("new-piff/"), "{shown}");
        assert!(shown.contains("Saved PIFF.md"), "{shown}");
    }

    #[test]
    fn editor_failures_are_shown_not_fatal() {
        let tmp = TempDir::new().unwrap();
        let mut app = App::open(tmp.path()).unwrap();
        app.edit_with(|_| bail!("editor `nope` exited with 127"));
        assert!(screen(&mut app).contains("editor `nope` exited with 127"));
    }

    #[test]
    fn files_open_in_the_editor_and_long_content_scrolls() {
        let tmp = TempDir::new().unwrap();
        let body: String = (1..=100).map(|n| format!("line {n}\n\n")).collect();
        fs::write(tmp.path().join("PIFF.md"), body).unwrap();
        fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let mut app = App::open(tmp.path()).unwrap();

        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Action::OpenFile(tmp.path().join("a.txt"))
        );

        assert!(screen(&mut app).contains("line 1"));
        press(&mut app, KeyCode::PageDown);
        let shown = screen(&mut app);
        assert!(
            !shown.contains("line 1\n") && shown.contains("line 10"),
            "{shown}"
        );
        for _ in 0..50 {
            press(&mut app, KeyCode::PageDown);
        }
        let shown = screen(&mut app);
        assert!(shown.contains("line 100"), "{shown}");
        press(&mut app, KeyCode::PageUp);
        assert!(app.scroll < app.max_scroll);
    }

    #[test]
    fn normalize_resolves_dots_lexically() {
        let base = std::path::absolute("/").unwrap();
        assert_eq!(normalize(&base.join("a/./b/../c")), base.join("a/c"));
    }
}
