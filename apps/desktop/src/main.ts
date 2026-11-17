import { api, type Crumb, type Entry, type FileView, type PiffView, type RootInfo } from "./api";

/** An item the main pane can show: a Piff (directory) or a plain file. */
interface Location {
  path: string;
  isPiff: boolean;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const els = {
  rootName: $<HTMLSpanElement>("root-name"),
  openFolder: $<HTMLButtonElement>("open-folder"),
  tree: $<HTMLElement>("tree"),
  back: $<HTMLButtonElement>("back"),
  forward: $<HTMLButtonElement>("forward"),
  breadcrumb: $<HTMLElement>("breadcrumb"),
  notice: $<HTMLDivElement>("notice"),
  content: $<HTMLElement>("content"),
};

const state = {
  root: null as RootInfo | null,
  current: null as Location | null,
  history: [] as Location[],
  historyIndex: -1,
  expanded: new Set<string>(),
  children: new Map<string, Entry[]>(),
};

// ---------- root ----------

function setRoot(root: RootInfo) {
  state.root = root;
  state.history = [];
  state.historyIndex = -1;
  state.expanded = new Set([root.path]);
  state.children.clear();
  els.rootName.textContent = root.name;
  els.rootName.title = root.path;
  void navigate({ path: root.path, isPiff: true });
}

async function pickRoot() {
  try {
    const root = await api.pickRoot();
    if (root) setRoot(root);
  } catch (err) {
    showNotice(String(err), "error");
  }
}

// ---------- navigation ----------

/** Bumped on every navigation, so only the latest one renders. */
let navigationSeq = 0;

async function navigate(loc: Location, push = true) {
  const seq = ++navigationSeq;
  // A slower, older request must not overwrite a newer one.
  const stale = () => seq !== navigationSeq;
  let view: PiffView | FileView;
  try {
    view = loc.isPiff ? await api.openPiff(loc.path) : await api.openFile(loc.path);
  } catch (err) {
    if (!stale()) showNotice(String(err), "error");
    return;
  }
  if (stale()) return;
  if (push) {
    state.history = state.history.slice(0, state.historyIndex + 1);
    state.history.push(loc);
    state.historyIndex = state.history.length - 1;
  }
  state.current = loc;
  hideNotice();

  // Reveal the item in the tree: expand every Piff above it, and the Piff
  // itself.
  const piffs = loc.isPiff ? view.breadcrumb : view.breadcrumb.slice(0, -1);
  for (const crumb of piffs) state.expanded.add(crumb.path);
  // A Piff that was just opened may have new children; refetch it.
  if (loc.isPiff) state.children.delete(loc.path);
  await loadExpanded();
  if (stale()) return;

  renderBreadcrumb(view.breadcrumb);
  if ("hasContent" in view) renderPiff(view);
  else renderFile(view);
  renderTree();
  updateHistoryButtons();
}

function go(delta: number) {
  const index = state.historyIndex + delta;
  const loc = state.history[index];
  if (!loc) return;
  state.historyIndex = index;
  void navigate(loc, false);
}

function updateHistoryButtons() {
  els.back.disabled = state.historyIndex <= 0;
  els.forward.disabled = state.historyIndex >= state.history.length - 1;
}

async function followLink(href: string) {
  if (!state.current) return;
  try {
    const target = await api.resolveLink(state.current.path, href);
    switch (target.kind) {
      case "piff":
        return navigate({ path: target.path, isPiff: true });
      case "file":
        return navigate({ path: target.path, isPiff: false });
      case "external":
        return api.openUrl(target.url);
      case "anchor":
        return scrollToAnchor(target.id);
    }
  } catch (err) {
    showNotice(String(err), "error");
  }
}

function scrollToAnchor(id: string) {
  if (!id) {
    els.content.scrollTo({ top: 0 });
    return;
  }
  const el = document.getElementById(id) ?? document.getElementsByName(id)[0];
  el?.scrollIntoView();
}

// ---------- main pane ----------

function renderBreadcrumb(crumbs: Crumb[]) {
  els.breadcrumb.replaceChildren();
  crumbs.forEach((crumb, i) => {
    if (i > 0) els.breadcrumb.append(el("span", "crumb-sep", "/"));
    const last = i === crumbs.length - 1;
    if (last) {
      els.breadcrumb.append(el("span", "crumb current", crumb.name));
    } else {
      const button = el("button", "crumb", crumb.name);
      button.title = crumb.path;
      button.addEventListener("click", () => navigate({ path: crumb.path, isPiff: true }));
      els.breadcrumb.append(button);
    }
  });
}

function renderPiff(view: PiffView) {
  els.content.className = "piff";
  els.content.innerHTML = view.html; // sanitized by the backend
  if (!view.hasContent) {
    els.content.prepend(
      el("p", "empty-piff", "This Piff has no PIFF.md yet. Its children are listed below."),
    );
  }
  if (view.warning) showNotice(view.warning, "warning");
  els.content.scrollTo({ top: 0 });
}

function renderFile(view: FileView) {
  const c = view.content;
  els.content.className = "file";
  els.content.replaceChildren();
  switch (c.kind) {
    case "markdown":
      els.content.innerHTML = c.html; // sanitized by the backend
      break;
    case "text":
      els.content.append(el("pre", "plain-text", c.text));
      break;
    case "binary":
    case "tooLarge": {
      const why =
        c.kind === "binary"
          ? `${view.name} is not a text file (${formatSize(c.size)}).`
          : `${view.name} is too large to preview (${formatSize(c.size)}).`;
      const box = el("div", "no-preview");
      box.append(el("p", "", why), openExternallyButton(view.path));
      els.content.append(box);
      return;
    }
  }
  const bar = el("div", "file-actions");
  bar.append(openExternallyButton(view.path));
  els.content.prepend(bar);
  els.content.scrollTo({ top: 0 });
}

function openExternallyButton(path: string) {
  const button = el("button", "open-external", "Open with default app");
  button.addEventListener("click", () =>
    api.openWithDefaultApp(path).catch((err) => showNotice(String(err), "error")),
  );
  return button;
}

function showNotice(message: string, kind: "error" | "warning") {
  els.notice.textContent = message;
  els.notice.className = kind;
  els.notice.hidden = false;
}

function hideNotice() {
  els.notice.hidden = true;
}

// ---------- sidebar tree ----------

/** Fetches children for every expanded Piff that hasn't been loaded yet. */
async function loadExpanded() {
  const missing = [...state.expanded].filter((p) => !state.children.has(p));
  await Promise.all(
    missing.map(async (path) => {
      try {
        state.children.set(path, await api.listChildren(path));
      } catch {
        // Gone or unreadable: drop it from the tree rather than fail.
        state.expanded.delete(path);
      }
    }),
  );
}

async function toggle(path: string) {
  if (state.expanded.has(path)) {
    state.expanded.delete(path);
  } else {
    state.expanded.add(path);
    await loadExpanded();
  }
  renderTree();
}

function renderTree() {
  els.tree.replaceChildren();
  if (!state.root) {
    els.tree.append(el("p", "tree-empty", "Open a folder to browse its Piffs."));
    return;
  }
  const rootEntry: Entry = { name: state.root.name, path: state.root.path, isPiff: true };
  const list = el("ul", "tree-list");
  list.setAttribute("role", "tree");
  list.append(treeItem(rootEntry, 0));
  els.tree.append(list);
  els.tree.querySelector(".tree-row.selected")?.scrollIntoView({ block: "nearest" });
}

function treeItem(entry: Entry, depth: number): HTMLLIElement {
  const li = el("li", "tree-item");
  li.setAttribute("role", "treeitem");
  const row = el("div", "tree-row");
  row.style.paddingLeft = `${depth * 14 + 6}px`;
  row.title = entry.path;
  if (state.current?.path === entry.path) row.classList.add("selected");

  const expanded = entry.isPiff && state.expanded.has(entry.path);
  const caret = el("span", "caret", entry.isPiff ? (expanded ? "▾" : "▸") : "");
  if (entry.isPiff) {
    li.setAttribute("aria-expanded", String(expanded));
    caret.addEventListener("click", (e) => {
      e.stopPropagation();
      void toggle(entry.path);
    });
  }
  row.append(
    caret,
    el("span", entry.isPiff ? "icon piff-icon" : "icon file-icon"),
    el("span", "label", entry.name),
  );
  row.addEventListener("click", () => navigate({ path: entry.path, isPiff: entry.isPiff }));
  li.append(row);

  if (expanded) {
    const kids = state.children.get(entry.path) ?? [];
    const ul = el("ul", "tree-list");
    ul.setAttribute("role", "group");
    for (const child of kids) ul.append(treeItem(child, depth + 1));
    li.append(ul);
  }
  return li;
}

// ---------- helpers ----------

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

function formatSize(bytes: number) {
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n < 10 && i > 0 ? n.toFixed(1) : Math.round(n)} ${units[i]}`;
}

// ---------- wiring ----------

els.openFolder.addEventListener("click", pickRoot);
els.back.addEventListener("click", () => go(-1));
els.forward.addEventListener("click", () => go(1));

// Links in rendered content navigate inside the app (or open in the browser
// for web links); the webview itself never follows them.
els.content.addEventListener("click", (e) => {
  const link = (e.target as Element).closest("a");
  if (!link) return;
  e.preventDefault();
  const href = link.getAttribute("href");
  if (href !== null) void followLink(href);
});

document.addEventListener("keydown", (e) => {
  const mod = e.metaKey || e.ctrlKey;
  if (mod && e.key.toLowerCase() === "o") {
    e.preventDefault();
    void pickRoot();
  } else if ((e.altKey && e.key === "ArrowLeft") || (mod && e.key === "[")) {
    e.preventDefault();
    go(-1);
  } else if ((e.altKey && e.key === "ArrowRight") || (mod && e.key === "]")) {
    e.preventDefault();
    go(1);
  }
});

// Mouse back/forward buttons.
document.addEventListener("mouseup", (e) => {
  if (e.button === 3) go(-1);
  if (e.button === 4) go(1);
});

renderTree();
api
  .restoreRoot()
  .then((root) => {
    if (root) setRoot(root);
    else renderWelcome();
  })
  .catch(() => renderWelcome());

function renderWelcome() {
  els.content.className = "welcome";
  els.content.replaceChildren();
  const button = el("button", "primary", "Open a folder…");
  button.addEventListener("click", pickRoot);
  els.content.append(
    el("h1", "", "NoDir"),
    el(
      "p",
      "",
      "Every folder is a Piff: it holds files and carries its own notes in PIFF.md. Open a folder to start browsing.",
    ),
    button,
  );
}
