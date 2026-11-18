// The children section inside the editor: shown rendered, never editable.
//
// The section's position comes from the backend (nodir-core finds it) and is
// then carried through edits by mapping. The editor never parses markers
// itself and never writes the section: saves send the whole text to
// nodir-core's writer, which regenerates it.
import {
  Annotation,
  EditorState,
  RangeSet,
  StateEffect,
  StateField,
  type ChangeSpec,
  type Extension,
  type Text,
} from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";

export interface SectionState {
  /** Where the section is in the document, or null if the document has
   * none yet (an empty Piff): then it is shown after the last line, where a
   * save will put it. */
  range: { from: number; to: number } | null;
  /** The section rendered to sanitized HTML by the backend. */
  html: string;
}

/** Marks transactions that bring in text from disk rather than typing. */
export const fromDisk = Annotation.define<boolean>();

/** Replaces the section state, e.g. after a save or a reload. */
export const setSection = StateEffect.define<SectionState>();

class ChildrenWidget extends WidgetType {
  constructor(readonly html: string) {
    super();
  }
  eq(other: ChildrenWidget) {
    return other.html === this.html;
  }
  toDOM() {
    const nav = document.createElement("nav");
    nav.className = "piff-children";
    nav.setAttribute("aria-label", "Children (maintained by NoDir)");
    nav.title = "Maintained by NoDir; not editable";
    nav.innerHTML = this.html; // sanitized by the backend
    return nav;
  }
  // CodeMirror handles mouse presses, so a click on the list puts the
  // cursor next to it rather than leaving the browser's caret inside it,
  // where typing would be lost. Clicks on child links still reach the
  // content pane's link handler.
  ignoreEvent(event: Event) {
    return event.type !== "mousedown";
  }
}

interface FieldValue extends SectionState {
  decorations: DecorationSet;
}

function decorate(state: SectionState, doc: Text): DecorationSet {
  const widget = new ChildrenWidget(state.html);
  if (!state.range) {
    return RangeSet.of([Decoration.widget({ widget, block: true, side: 1 }).range(doc.length)]);
  }
  // A block replacement must end at a line end; trailing spaces after the
  // end marker are hidden with the section.
  const to = doc.lineAt(state.range.to).to;
  return RangeSet.of([
    Decoration.replace({ widget, block: true }).range(state.range.from, to),
  ]);
}

export const sectionField = StateField.define<FieldValue>({
  create: (state) => ({ range: null, html: "", decorations: decorate({ range: null, html: "" }, state.doc) }),
  update(value, tr) {
    let next: SectionState = value;
    for (const effect of tr.effects) if (effect.is(setSection)) next = effect.value;
    if (next === value) {
      if (!tr.docChanged) return value;
      // Typing right before the section goes before it, typing right after
      // it goes after it.
      next = {
        html: value.html,
        range: value.range && {
          from: tr.changes.mapPos(value.range.from, 1),
          to: tr.changes.mapPos(value.range.to, -1),
        },
      };
    }
    return { ...next, decorations: decorate(next, tr.state.doc) };
  },
  provide: (field) => [
    EditorView.decorations.from(field, (v) => v.decorations),
    EditorView.atomicRanges.of((view) => view.state.field(field).decorations),
  ],
});

/** Edits never change text inside the section; the parts of an edit that
 * reach into it are dropped. */
const readOnly = EditorState.changeFilter.of((tr) => {
  const range = tr.startState.field(sectionField).range;
  return range ? [range.from, range.to] : true;
});

/**
 * The markers only count on lines of their own. If an edit leaves text
 * right before the start marker or right after the end marker on the same
 * line, a line break is added to keep them apart.
 */
const keepMarkersOnTheirOwnLines = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged || tr.annotation(fromDisk)) return tr;
  const before = tr.startState.field(sectionField).range;
  if (!before) return tr;
  const from = tr.changes.mapPos(before.from, 1);
  const to = tr.changes.mapPos(before.to, -1);
  const doc = tr.newDoc;
  const sep = tr.startState.lineBreak;
  const fixes: ChangeSpec[] = [];
  if (doc.lineAt(from).from !== from) fixes.push({ from, insert: sep });
  if (doc.sliceString(to, doc.lineAt(to).to).trim() !== "") fixes.push({ from: to, insert: sep });
  if (fixes.length === 0) return tr;
  return [tr, { changes: fixes, sequential: true }];
});

export function childrenSection(): Extension {
  return [sectionField, readOnly, keepMarkersOnTheirOwnLines];
}
