// Typed wrappers for the Tauri commands in src-tauri/src/lib.rs. All Piff
// logic lives in the backend (nodir-core); the frontend only draws.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface RootInfo {
  path: string;
  name: string;
}

export interface Crumb {
  name: string;
  path: string;
}

export interface Entry {
  name: string;
  path: string;
  isPiff: boolean;
}

/** A Piff's content for the editor. Offsets are JavaScript string offsets. */
export interface PiffDoc {
  /** Whether PIFF.md exists; if not, `text` is empty. */
  exists: boolean;
  text: string;
  section: { from: number; to: number } | null;
  /** The children section rendered to sanitized HTML. */
  childrenHtml: string;
  lineEnding: "\n" | "\r\n";
}

export type SaveResult = { kind: "saved"; doc: PiffDoc } | { kind: "conflict"; doc: PiffDoc };

export interface PiffView {
  path: string;
  name: string;
  doc: PiffDoc;
  breadcrumb: Crumb[];
  warning: string | null;
}

export type FileContent =
  | { kind: "markdown"; html: string }
  | { kind: "text"; text: string }
  | { kind: "binary"; size: number }
  | { kind: "tooLarge"; size: number };

export interface FileView {
  path: string;
  name: string;
  breadcrumb: Crumb[];
  content: FileContent;
}

export type Target =
  | { kind: "piff"; path: string }
  | { kind: "file"; path: string }
  | { kind: "external"; url: string }
  | { kind: "anchor"; id: string };

export const api = {
  restoreRoot: () => invoke<RootInfo | null>("restore_root"),
  pickRoot: () => invoke<RootInfo | null>("pick_root"),
  listChildren: (path: string) => invoke<Entry[]>("list_children", { path }),
  openPiff: (path: string) => invoke<PiffView>("open_piff", { path }),
  readPiff: (path: string) => invoke<PiffDoc>("read_piff", { path }),
  savePiff: (path: string, text: string, expected: string | null) =>
    invoke<SaveResult>("save_piff", { path, text, expected }),
  watchPiff: (path: string | null) => invoke<void>("watch_piff", { path }),
  openInExternalEditor: (path: string) => invoke<void>("open_in_external_editor", { path }),
  onPiffChanged: (f: (path: string) => void): Promise<UnlistenFn> =>
    listen<{ path: string }>("piff-changed", (e) => f(e.payload.path)),
  openFile: (path: string) => invoke<FileView>("open_file", { path }),
  resolveLink: (from: string, href: string) =>
    invoke<Target>("resolve_link", { from, href }),
  openWithDefaultApp: (path: string) =>
    invoke<void>("open_with_default_app", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};
