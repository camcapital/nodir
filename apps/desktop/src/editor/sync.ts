// Pure helpers for keeping the editor in step with PIFF.md on disk.
//
// The backend speaks in JavaScript string offsets (UTF-16 code units) over
// the file's exact text. CodeMirror speaks in document positions, where a
// line break counts as one character whatever the file's line ending is. So
// for a CRLF file every "\r\n" before a point shrinks its position by one.
// Everything here works on raw strings and returns CodeMirror change sets in
// document positions.
import { ChangeSet, type ChangeSpec } from "@codemirror/state";

export interface Span {
  from: number;
  to: number;
}

/** The document position of string offset `offset` in `text`. */
export function toDocPos(text: string, offset: number, sep: string): number {
  if (sep.length === 1) return offset;
  let pos = offset;
  for (let i = text.indexOf(sep); i !== -1 && i + sep.length <= offset; i = text.indexOf(sep, i + sep.length)) {
    pos -= sep.length - 1;
  }
  return pos;
}

/** `span` (string offsets in `text`) in document positions. */
export function spanToDoc(text: string, span: Span, sep: string): Span {
  return { from: toDocPos(text, span.from, sep), to: toDocPos(text, span.to, sep) };
}

/** Length of `text` as a CodeMirror document. */
export function docLength(text: string, sep: string): number {
  return toDocPos(text, text.length, sep);
}

/**
 * The change turning `a` into `b`, as one replacement between their common
 * prefix and suffix, or none if they are equal. Within `aFrom..aTo` of `a`
 * and `bFrom..bTo` of `b` only, if given.
 */
function diffRange(
  a: string,
  b: string,
  sep: string,
  aFrom = 0,
  aTo = a.length,
  bFrom = 0,
  bTo = b.length,
): ChangeSpec | null {
  let start = 0;
  const max = Math.min(aTo - aFrom, bTo - bFrom);
  while (start < max && a.charCodeAt(aFrom + start) === b.charCodeAt(bFrom + start)) start++;
  let end = 0;
  while (
    end < max - start &&
    a.charCodeAt(aTo - 1 - end) === b.charCodeAt(bTo - 1 - end)
  ) {
    end++;
  }
  if (start === aTo - aFrom && start === bTo - bFrom) return null;
  // Never split a multi-character line break (or a surrogate pair): move
  // the boundaries outwards until neither string has one straddling them.
  const splits = (s: string, i: number) =>
    i > 0 &&
    i < s.length &&
    ((sep.length > 1 && s.startsWith(sep, i - 1)) ||
      (s.charCodeAt(i) >= 0xdc00 && s.charCodeAt(i) <= 0xdfff));
  while (start > 0 && (splits(a, aFrom + start) || splits(b, bFrom + start))) start--;
  while (end > 0 && (splits(a, aTo - end) || splits(b, bTo - end))) end--;
  return {
    from: toDocPos(a, aFrom + start, sep),
    to: toDocPos(a, aTo - end, sep),
    insert: b.slice(bFrom + start, bTo - end),
  };
}

/**
 * The changes turning `a` into `b`. When both have a children section, the
 * text before it, the section, and the text after it are compared
 * separately, so an edit at the top and a regenerated section at the bottom
 * stay two small changes instead of one that covers the whole file.
 */
export function diff(
  a: string,
  aSection: Span | null,
  b: string,
  bSection: Span | null,
  sep: string,
): ChangeSet {
  const specs: ChangeSpec[] = [];
  const push = (spec: ChangeSpec | null) => {
    if (spec) specs.push(spec);
  };
  if (aSection && bSection) {
    push(diffRange(a, b, sep, 0, aSection.from, 0, bSection.from));
    push(diffRange(a, b, sep, aSection.from, aSection.to, bSection.from, bSection.to));
    push(diffRange(a, b, sep, aSection.to, a.length, bSection.to, b.length));
  } else {
    push(diffRange(a, b, sep));
  }
  return ChangeSet.of(specs, docLength(a, sep), sep);
}

/**
 * Whether two change sets over the same document touch the same text.
 * Changes that only meet at a point count as touching: an insertion right
 * where someone else edited is a conflict too.
 */
export function overlaps(x: ChangeSet, y: ChangeSet): boolean {
  const ranges: Span[] = [];
  x.iterChangedRanges((from, to) => ranges.push({ from, to }));
  let hit = false;
  y.iterChangedRanges((from, to) => {
    if (ranges.some((r) => r.from <= to && from <= r.to)) hit = true;
  });
  return hit;
}
