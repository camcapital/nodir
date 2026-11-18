// Obsidian-style live preview: markdown renders in place as you type, and a
// construct's raw syntax shows only while the cursor is on it.
//
// Inline constructs (emphasis, code, links) reveal their syntax when the
// selection touches them; block constructs (headings, quotes, list markers,
// code fences, rules) when the selection is on their line. All of this is
// decoration: the document text is never changed by rendering.
import { syntaxTree } from "@codemirror/language";
import type { Range } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";

const hide = Decoration.replace({});
const line = (cls: string) => Decoration.line({ class: cls });
const mark = (cls: string) => Decoration.mark({ class: cls });

class BulletWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const span = document.createElement("span");
    span.className = "cm-lp-bullet";
    span.textContent = "•";
    return span;
  }
}

class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean) {
    super();
  }
  eq(other: CheckboxWidget) {
    return other.checked === this.checked;
  }
  toDOM() {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-lp-task";
    box.checked = this.checked;
    return box;
  }
  ignoreEvent() {
    return false;
  }
}

class RuleWidget extends WidgetType {
  eq() {
    return true;
  }
  toDOM() {
    const span = document.createElement("span");
    span.className = "cm-lp-hr";
    return span;
  }
}

function build(view: EditorView): DecorationSet {
  const { state } = view;
  const doc = state.doc;
  const deco: Range<Decoration>[] = [];
  // When the editor isn't focused, everything renders. Focus is judged by
  // the active element, not `view.hasFocus`, which also needs the window to
  // be focused: keys can still arrive without that, and typing into text
  // that is hidden would make the browser's DOM disagree with the document.
  const focused = view.root.activeElement === view.contentDOM;
  const ranges = focused ? state.selection.ranges : [];
  const touches = (from: number, to: number) => ranges.some((r) => r.from <= to && r.to >= from);
  const onLines = (from: number, to: number) => touches(doc.lineAt(from).from, doc.lineAt(to).to);

  /** Hides a block marker, and the space after it. */
  const hideMarker = (node: SyntaxNode) => {
    const end = doc.sliceString(node.to, node.to + 1) === " " ? node.to + 1 : node.to;
    if (end > node.from) deco.push(hide.range(node.from, end));
  };
  const children = (node: SyntaxNode, name: string) => {
    const out: SyntaxNode[] = [];
    for (let c = node.firstChild; c; c = c.nextSibling) if (c.name === name) out.push(c);
    return out;
  };
  const eachLine = (from: number, to: number, f: (l: { from: number; to: number; number: number }) => void) => {
    for (let pos = from; pos <= to; ) {
      const l = doc.lineAt(pos);
      f(l);
      pos = l.to + 1;
    }
  };

  for (const visible of view.visibleRanges) {
    syntaxTree(state).iterate({
      from: visible.from,
      to: visible.to,
      enter: (ref) => {
        const node = ref.node;
        const name = node.name;
        const heading = /^ATXHeading(\d)$/.exec(name);
        if (heading) {
          deco.push(line(`cm-lp-h cm-lp-h${heading[1]}`).range(doc.lineAt(node.from).from));
          if (!onLines(node.from, node.to)) for (const m of children(node, "HeaderMark")) hideMarker(m);
          return;
        }
        const setext = /^SetextHeading(\d)$/.exec(name);
        if (setext) {
          eachLine(node.from, node.to, (l) => deco.push(line(`cm-lp-h cm-lp-h${setext[1]}`).range(l.from)));
          for (const m of children(node, "HeaderMark")) deco.push(mark("cm-lp-syntax").range(m.from, m.to));
          return;
        }
        switch (name) {
          case "Emphasis":
          case "StrongEmphasis":
          case "Strikethrough": {
            const cls = { Emphasis: "cm-lp-em", StrongEmphasis: "cm-lp-strong", Strikethrough: "cm-lp-strike" }[name];
            deco.push(mark(cls).range(node.from, node.to));
            if (!touches(node.from, node.to)) {
              const marks = name === "Strikethrough" ? "StrikethroughMark" : "EmphasisMark";
              for (const m of children(node, marks)) deco.push(hide.range(m.from, m.to));
            }
            return;
          }
          case "InlineCode":
            deco.push(mark("cm-lp-code").range(node.from, node.to));
            if (!touches(node.from, node.to)) for (const m of children(node, "CodeMark")) deco.push(hide.range(m.from, m.to));
            return false;
          case "Link":
            linkDeco(node);
            return;
          case "Autolink":
          case "URL":
            if (node.parent?.name === "Link" || node.parent?.name === "Image") return;
            deco.push(
              Decoration.mark({
                class: "cm-lp-link",
                attributes: { "data-href": doc.sliceString(node.from, node.to).replace(/^<|>$/g, "") },
              }).range(node.from, node.to),
            );
            return false;
          case "Blockquote":
            eachLine(node.from, node.to, (l) => deco.push(line("cm-lp-quote").range(l.from)));
            return;
          case "QuoteMark":
            if (!onLines(node.from, node.to)) hideMarker(node);
            return;
          case "ListMark": {
            const bullet = node.parent?.parent?.name === "BulletList";
            const task = node.parent?.getChild("Task");
            if (bullet && !task && !onLines(node.from, node.to)) {
              deco.push(Decoration.replace({ widget: new BulletWidget() }).range(node.from, node.to));
            } else if (bullet && task) {
              // A task's bullet gives way to its checkbox.
              if (!onLines(node.from, node.to)) hideMarker(node);
            } else {
              deco.push(mark("cm-lp-listmark").range(node.from, node.to));
            }
            return;
          }
          case "TaskMarker":
            if (!onLines(node.from, node.to)) {
              const checked = /x/i.test(doc.sliceString(node.from, node.to));
              deco.push(Decoration.replace({ widget: new CheckboxWidget(checked) }).range(node.from, node.to));
            }
            return;
          case "FencedCode": {
            const first = doc.lineAt(node.from);
            const last = doc.lineAt(node.to);
            const active = onLines(node.from, node.to);
            eachLine(node.from, node.to, (l) => {
              let cls = "cm-lp-codeblock";
              if (l.number === first.number) cls += " cm-lp-codeblock-first";
              if (l.number === last.number) cls += " cm-lp-codeblock-last";
              deco.push(line(cls).range(l.from));
            });
            if (!active) {
              // Hide the fence lines' text; the lines stay, as padding.
              if (first.to > first.from) deco.push(hide.range(first.from, first.to));
              const close = node.lastChild;
              if (last.number !== first.number && close?.name === "CodeMark" && last.to > last.from) {
                deco.push(hide.range(last.from, last.to));
              }
            }
            return false;
          }
          case "CodeBlock":
            eachLine(node.from, node.to, (l) => deco.push(line("cm-lp-codeblock cm-lp-indented").range(l.from)));
            return false;
          case "HorizontalRule":
            if (!onLines(node.from, node.to)) {
              deco.push(Decoration.replace({ widget: new RuleWidget() }).range(node.from, node.to));
            }
            return;
        }
        return;
      },
    });
  }

  function linkDeco(node: SyntaxNode) {
    const marks = children(node, "LinkMark");
    const url = node.getChild("URL");
    const open = marks[0];
    const close = marks.find((m) => doc.sliceString(m.from, m.to) === "]");
    if (!open || !close || !url) {
      deco.push(mark("cm-lp-link-src").range(node.from, node.to));
      return;
    }
    const href = doc.sliceString(url.from, url.to).replace(/^<|>$/g, "");
    if (touches(node.from, node.to) || close.from <= open.to) {
      deco.push(mark("cm-lp-link-src").range(node.from, node.to));
      return;
    }
    deco.push(hide.range(node.from, open.to));
    deco.push(
      Decoration.mark({ class: "cm-lp-link", attributes: { "data-href": href, title: href } }).range(
        open.to,
        close.from,
      ),
    );
    deco.push(hide.range(close.from, node.to));
  }

  return Decoration.set(deco, true);
}

/** Live preview, with `onLink` called when a rendered link is clicked. */
export function livePreview(onLink: (href: string) => void) {
  return [
    ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
          this.decorations = build(view);
        }
        update(u: ViewUpdate) {
          if (
            u.docChanged ||
            u.selectionSet ||
            u.viewportChanged ||
            u.focusChanged ||
            syntaxTree(u.startState) !== syntaxTree(u.state)
          ) {
            this.decorations = build(u.view);
          }
        }
      },
      { decorations: (v) => v.decorations },
    ),
    EditorView.domEventHandlers({
      mousedown(event, view) {
        const target = event.target as HTMLElement;
        // A rendered link opens; to edit it, move the cursor in with the
        // keyboard, or click just outside it.
        const link = target.closest<HTMLElement>(".cm-lp-link");
        if (link && event.button === 0 && !event.shiftKey) {
          const href = link.dataset.href;
          if (href) {
            event.preventDefault();
            onLink(href);
            return true;
          }
        }
        if (target instanceof HTMLInputElement && target.classList.contains("cm-lp-task")) {
          event.preventDefault();
          const pos = view.posAtDOM(target);
          const text = view.state.sliceDoc(pos, pos + 3);
          if (/^\[[ xX]\]$/.test(text)) {
            view.dispatch({ changes: { from: pos + 1, to: pos + 2, insert: text[1] === " " ? "x" : " " } });
          }
          return true;
        }
        return false;
      },
    }),
  ];
}
