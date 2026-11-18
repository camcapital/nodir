//! The NoDir desktop app: browse, read and edit Piffs.
//!
//! The webview only draws. Everything about Piffs goes through `nodir-core`
//! via the Tauri commands below, and every path the frontend passes in is
//! checked against the root folder the user opened.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

mod browse;
mod edit;
mod render;
mod watch;

use browse::{Entry, FileView, PiffView, Result, Root, RootInfo, Target};
use edit::{PiffDoc, SaveResult};

/// The root folder currently open, if any, and the watch on the `PIFF.md`
/// being edited.
#[derive(Default)]
struct AppState {
    root: Mutex<Option<Root>>,
    watch: Mutex<Option<watch::PiffWatch>>,
}

impl AppState {
    fn root(&self) -> Result<Root> {
        self.root
            .lock()
            .map_err(|e| e.to_string())?
            .clone()
            .ok_or_else(|| "No folder is open".to_string())
    }

    fn set_root(&self, app: &AppHandle, root: Root) -> Result<RootInfo> {
        let info = root.info();
        *self.root.lock().map_err(|e| e.to_string())? = Some(root);
        *self.watch.lock().map_err(|e| e.to_string())? = None;
        // Remembering the root is a convenience; failing to is not an error.
        if let Some(file) = last_root_file(app) {
            if let Some(dir) = file.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(file, &info.path);
        }
        Ok(info)
    }
}

/// Where the last opened root is remembered, in the app's config directory.
fn last_root_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("last-root"))
}

/// Reopens the root from the last session, if it still exists.
#[tauri::command]
fn restore_root(app: AppHandle, state: State<'_, AppState>) -> Option<RootInfo> {
    let saved = fs::read_to_string(last_root_file(&app)?).ok()?;
    let root = Root::open(saved.trim()).ok()?;
    state.set_root(&app, root).ok()
}

/// Asks the user for a folder and opens it as the root. `None` if they
/// cancelled.
#[tauri::command]
async fn pick_root(app: AppHandle, state: State<'_, AppState>) -> Result<Option<RootInfo>> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    state.set_root(&app, Root::open(path)?).map(Some)
}

#[tauri::command]
fn list_children(state: State<'_, AppState>, path: String) -> Result<Vec<Entry>> {
    state.root()?.children(&path)
}

#[tauri::command]
fn open_piff(state: State<'_, AppState>, path: String) -> Result<PiffView> {
    state.root()?.open_piff(&path)
}

/// Reads a Piff's content for the editor without writing anything.
#[tauri::command]
fn read_piff(state: State<'_, AppState>, path: String) -> Result<PiffDoc> {
    edit::read(&state.root()?.piff(&path)?)
}

/// Saves the editor's text through `nodir-core`'s writer, unless `PIFF.md`
/// changed on disk since the editor last saw it (`expected`).
#[tauri::command]
fn save_piff(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected: Option<String>,
) -> Result<SaveResult> {
    edit::save(&state.root()?.piff(&path)?, &text, expected.as_deref())
}

/// The payload of [`watch::EVENT`].
#[derive(Debug, Clone, Serialize)]
struct PiffChanged {
    /// The Piff directory, as the frontend passed it in.
    path: String,
}

/// Watches the `PIFF.md` of the Piff at `path` for external changes, in
/// place of any earlier watch. `None` stops watching.
#[tauri::command]
fn watch_piff(app: AppHandle, state: State<'_, AppState>, path: Option<String>) -> Result<()> {
    let mut current = state.watch.lock().map_err(|e| e.to_string())?;
    *current = None;
    if let Some(path) = path {
        let piff = state.root()?.piff(&path)?;
        *current = Some(watch::watch(piff.dir(), move || {
            let _ = app.emit(watch::EVENT, PiffChanged { path: path.clone() });
        })?);
    }
    Ok(())
}

/// Opens a Piff's `PIFF.md` in the OS default app for markdown, creating
/// the file first if it does not exist yet.
#[tauri::command]
fn open_in_external_editor(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<()> {
    let piff = state.root()?.piff(&path)?;
    edit::ensure_content_file(&piff)?;
    app.opener()
        .open_path(piff.content_path().to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_file(state: State<'_, AppState>, path: String) -> Result<FileView> {
    state.root()?.open_file(&path)
}

#[tauri::command]
fn resolve_link(state: State<'_, AppState>, from: String, href: String) -> Result<Target> {
    state.root()?.resolve_link(&from, &href)
}

/// Opens a file inside the root with the OS default app.
#[tauri::command]
fn open_with_default_app(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<()> {
    let path = state.root()?.contain(&path)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Opens an http(s) or mailto link in the default browser or mail client.
#[tauri::command]
fn open_url(app: AppHandle, state: State<'_, AppState>, url: String) -> Result<()> {
    // Only URLs the link resolver accepts as external may be opened.
    match state.root()?.resolve_link("", &url)? {
        Target::External { url } => app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string()),
        _ => Err(format!("{url} is not a web link")),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            restore_root,
            pick_root,
            list_children,
            open_piff,
            read_piff,
            save_piff,
            watch_piff,
            open_in_external_editor,
            open_file,
            resolve_link,
            open_with_default_app,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the NoDir desktop app");
}
