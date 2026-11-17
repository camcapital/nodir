// Typed wrappers for the Tauri commands in src-tauri/src/lib.rs. All Piff
// logic lives in the backend (nodir-core); the frontend only draws.
import { invoke } from "@tauri-apps/api/core";

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

export interface PiffView {
  path: string;
  name: string;
  hasContent: boolean;
  html: string;
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
  openFile: (path: string) => invoke<FileView>("open_file", { path }),
  resolveLink: (from: string, href: string) =>
    invoke<Target>("resolve_link", { from, href }),
  openWithDefaultApp: (path: string) =>
    invoke<void>("open_with_default_app", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};
