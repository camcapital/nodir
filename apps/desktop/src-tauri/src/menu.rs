//! The macOS app menu, with a Quit that saves first.
//!
//! The stock Quit item sends `terminate:`, which ends the process without a
//! close request reaching any window, so the webview never gets a chance to
//! save edits still waiting for autosave. NoDir's Quit asks each window to
//! close instead: the webview's close handler saves (or refuses, if the
//! edits can't be saved), and the app exits when its last window is gone.

use tauri::{AppHandle, Manager, Runtime};

/// The menu id of NoDir's Quit item.
pub const QUIT: &str = "nodir-quit";

/// Closes every window, each through its normal close request, or exits
/// right away if there are none.
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    let windows = app.webview_windows();
    if windows.is_empty() {
        app.exit(0);
    }
    for window in windows.values() {
        let _ = window.close();
    }
}

/// The standard macOS menus, with NoDir's Quit in place of the stock one.
#[cfg(target_os = "macos")]
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

    let name = app.package_info().name.clone();
    let quit = MenuItemBuilder::with_id(QUIT, format!("Quit {name}"))
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;
    let app_menu = SubmenuBuilder::new(app, &name)
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&quit)
        .build()?;
    let file = SubmenuBuilder::new(app, "File").close_window().build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;
    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &window])
        .build()
}
