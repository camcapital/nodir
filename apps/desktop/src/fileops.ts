// File operations in the UI: the tree's context menu, the dialogs that ask
// for a name or a confirmation, and the keyboard shortcuts. The operations
// themselves run in the backend (nodir-core), which also regenerates the
// parent Piff's children section.
import { api, type Done } from "./api";

/** An item an operation can act on. `parent` is null only for the root. */
export interface Item {
  path: string;
  name: string;
  isPiff: boolean;
  parent: string | null;
}

export type Change =
  | { kind: "created"; path: string; isPiff: boolean; done: Done }
  | { kind: "renamed"; from: string; to: string; done: Done }
  | { kind: "trashed"; path: string; done: Done };

/** What the rest of the app provides to this module. */
export interface Host {
  /** The selected item, which keyboard shortcuts act on. */
  selected(): Item | null;
  /** Called before `path` is renamed or trashed: saves pending edits to
   * anything under it. False if they could not be saved, which cancels the
   * operation. */
  beforeMove(path: string): Promise<boolean>;
  /** Refreshes the tree and the main pane after a change. */
  applied(change: Change): Promise<void>;
}

const isMac = navigator.userAgent.includes("Mac");
const trashName = navigator.userAgent.includes("Windows") ? "Recycle Bin" : "Trash";
const mod = isMac ? "⌘" : "Ctrl+";

let host: Host;

export function initFileOps(h: Host) {
  host = h;
  document.addEventListener("keydown", onKeyDown);
  document.addEventListener("mousedown", (e) => {
    if (menu && !menu.contains(e.target as Node)) closeMenu();
  });
  window.addEventListener("blur", closeMenu);
  window.addEventListener("resize", closeMenu);
}

// ---------- operations ----------

/** The Piff a new item goes into: the item itself, or a file's parent. */
function targetPiff(item: Item): string | null {
  return item.isPiff ? item.path : item.parent;
}

export async function newPiff(item: Item) {
  const parent = targetPiff(item);
  if (parent === null) return;
  const done = await ask({
    title: "New Piff",
    submit: "Create",
    fields: [
      { label: "Name", value: "" },
      { label: "Title (optional, starts its PIFF.md)", value: "" },
    ],
    run: ([name, title]) => api.createPiff(parent, name, title.trim() ? title : null),
  });
  if (done) await host.applied({ kind: "created", path: done.path, isPiff: true, done });
}

export async function newFile(item: Item) {
  const parent = targetPiff(item);
  if (parent === null) return;
  const done = await ask({
    title: "New file",
    submit: "Create",
    fields: [{ label: "Name", value: "", placeholder: "notes.md" }],
    run: ([name]) => api.createFile(parent, name),
  });
  if (done) await host.applied({ kind: "created", path: done.path, isPiff: false, done });
}

export async function rename(item: Item) {
  if (item.parent === null) return;
  if (!(await host.beforeMove(item.path))) return;
  const done = await ask({
    title: `Rename ${item.isPiff ? "Piff" : "file"}`,
    submit: "Rename",
    fields: [{ label: "Name", value: item.name, selectStem: !item.isPiff }],
    run: ([name]) => api.renameEntry(item.path, name),
  });
  if (done) await host.applied({ kind: "renamed", from: item.path, to: done.path, done });
}

export async function trash(item: Item) {
  if (item.parent === null) return;
  if (!(await host.beforeMove(item.path))) return;
  const what = item.isPiff ? "Piff and everything in it" : "file";
  const done = await ask({
    title: `Move “${item.name}” to the ${trashName}?`,
    message: `The ${what} can be restored from the ${trashName}.`,
    submit: `Move to ${trashName}`,
    danger: true,
    fields: [],
    run: () => api.trashEntry(item.path),
  });
  if (done) await host.applied({ kind: "trashed", path: item.path, done });
}

// ---------- keyboard ----------

function isEditable(target: EventTarget | null) {
  const el = target as HTMLElement | null;
  return !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
}

function onKeyDown(e: KeyboardEvent) {
  if (menu && e.key === "Escape") {
    e.preventDefault();
    closeMenu();
    return;
  }
  // Leave typing alone (the editor, dialog fields), and do nothing while a
  // dialog is open.
  if (dialogOpen || isEditable(e.target)) return;
  const item = host.selected();
  if (!item) return;
  const cmd = e.metaKey || e.ctrlKey;
  const key = e.key.toLowerCase();
  let action: ((item: Item) => Promise<void>) | null = null;
  if (cmd && e.shiftKey && key === "n") action = newPiff;
  else if (cmd && !e.shiftKey && !e.altKey && key === "n") action = newFile;
  else if (e.key === "F2") action = rename;
  else if ((cmd && e.key === "Backspace") || (!cmd && e.key === "Delete")) action = trash;
  if (!action) return;
  e.preventDefault();
  closeMenu();
  void action(item);
}

// ---------- context menu ----------

let menu: HTMLElement | null = null;

/** Opens the context menu for `item` at the pointer. */
export function openMenu(e: MouseEvent, item: Item) {
  e.preventDefault();
  closeMenu();
  const entries: [string, string, (item: Item) => Promise<void>, boolean?][] = [
    ["New Piff…", `${mod}${isMac ? "⇧" : "Shift+"}N`, newPiff],
    ["New file…", `${mod}N`, newFile],
  ];
  if (item.parent !== null) {
    entries.push(
      ["Rename…", "F2", rename],
      [`Move to ${trashName}`, isMac ? "⌘⌫" : "Del", trash, true],
    );
  }
  menu = el("div", "context-menu");
  menu.setAttribute("role", "menu");
  for (const [label, shortcut, action, danger] of entries) {
    const button = el("button", danger ? "danger" : "");
    button.type = "button";
    button.setAttribute("role", "menuitem");
    button.append(el("span", "", label), el("kbd", "", shortcut));
    button.addEventListener("click", () => {
      closeMenu();
      void action(item);
    });
    menu.append(button);
  }
  document.body.append(menu);
  // Keep the menu on screen.
  const { innerWidth: w, innerHeight: h } = window;
  const rect = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(e.clientX, w - rect.width - 4)}px`;
  menu.style.top = `${Math.min(e.clientY, h - rect.height - 4)}px`;
  menu.querySelector("button")?.focus();
  menu.addEventListener("keydown", (ev) => {
    const buttons = [...menu!.querySelectorAll("button")];
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (ev.key === "ArrowDown") buttons[(i + 1) % buttons.length].focus();
    else if (ev.key === "ArrowUp") buttons[(i - 1 + buttons.length) % buttons.length].focus();
    else return;
    ev.preventDefault();
  });
}

function closeMenu() {
  menu?.remove();
  menu = null;
}

// ---------- dialog ----------

interface Field {
  label: string;
  value: string;
  placeholder?: string;
  /** Select the name without its extension, as Finder does. */
  selectStem?: boolean;
}

interface Ask {
  title: string;
  message?: string;
  submit: string;
  danger?: boolean;
  fields: Field[];
  /** Runs the operation. A rejection is shown in the dialog, which stays
   * open so the user can fix the name. */
  run: (values: string[]) => Promise<Done>;
}

let dialogOpen = false;

/** Shows a modal dialog; resolves with the operation's result, or null if
 * the user cancelled. */
function ask(opts: Ask): Promise<Done | null> {
  dialogOpen = true;
  const previousFocus = document.activeElement as HTMLElement | null;
  const backdrop = el("div", "modal-backdrop");
  const form = el("form", "modal");
  form.setAttribute("role", "dialog");
  form.setAttribute("aria-modal", "true");
  form.append(el("h2", "", opts.title));
  if (opts.message) form.append(el("p", "", opts.message));
  const inputs = opts.fields.map((f) => {
    const label = el("label", "", f.label);
    const input = el("input");
    input.type = "text";
    input.value = f.value;
    input.placeholder = f.placeholder ?? "";
    input.spellcheck = false;
    input.autocomplete = "off";
    label.append(input);
    form.append(label);
    return input;
  });
  const error = el("p", "modal-error");
  error.hidden = true;
  const cancel = el("button", "", "Cancel");
  cancel.type = "button";
  const submit = el("button", opts.danger ? "primary danger" : "primary", opts.submit);
  submit.type = "submit";
  const buttons = el("div", "modal-buttons");
  buttons.append(cancel, submit);
  form.append(error, buttons);
  backdrop.append(form);
  document.body.append(backdrop);

  const first = inputs[0];
  if (first) {
    first.focus();
    const dot = first.value.lastIndexOf(".");
    const stem = opts.fields[0].selectStem && dot > 0 ? dot : first.value.length;
    first.setSelectionRange(0, stem);
  } else {
    submit.focus();
  }

  return new Promise((resolve) => {
    // While the operation runs, the dialog can't be dismissed: it would
    // finish on disk anyway, and the tree would not be refreshed.
    let busy = false;
    const close = (result: Done | null) => {
      backdrop.remove();
      dialogOpen = false;
      previousFocus?.focus();
      resolve(result);
    };
    cancel.addEventListener("click", () => {
      if (!busy) close(null);
    });
    backdrop.addEventListener("mousedown", (e) => {
      if (e.target === backdrop && !busy) close(null);
    });
    form.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        if (!busy) close(null);
      }
    });
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (busy) return;
      busy = true;
      submit.disabled = cancel.disabled = true;
      try {
        close(await opts.run(inputs.map((i) => i.value)));
      } catch (err) {
        error.textContent = String(err);
        error.hidden = false;
        busy = false;
        submit.disabled = cancel.disabled = false;
        first?.focus();
      }
    });
  });
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = "",
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
