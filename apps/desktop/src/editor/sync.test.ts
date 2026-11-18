import { describe, expect, it } from "vitest";
import { Text } from "@codemirror/state";

import { diff, overlaps, toDocPos } from "./sync";

const apply = (a: string, b: string, sep: string, aSec = null, bSec = null) => {
  const doc = Text.of(a.split(sep));
  return diff(a, aSec, b, bSec, sep).apply(doc).sliceString(0, undefined, sep);
};

describe("diff", () => {
  it("turns one text into the other", () => {
    for (const [a, b] of [
      ["hello world", "hello brave world"],
      ["abc", ""],
      ["", "abc"],
      ["same", "same"],
      ["aaa", "aaaa"],
    ]) {
      expect(apply(a, b, "\n")).toBe(b);
    }
  });

  it("is a single small change", () => {
    const change = diff("one two three", null, "one 2 three", null, "\n");
    const ranges: number[][] = [];
    change.iterChanges((fa, ta, fb, tb) => ranges.push([fa, ta, fb, tb]));
    expect(ranges).toEqual([[4, 7, 4, 5]]);
  });

  it("keeps CRLF line breaks whole", () => {
    const a = "a\r\nb\r\nc";
    for (const b of ["a\r\nB\r\nc", "a\r\nb\r\n\r\nc", "a\rb\r\nc", "a\r\n\r\nb\r\nc"]) {
      expect(apply(a, b, "\r\n")).toBe(b);
    }
  });

  it("diffs around the children section separately", () => {
    const a = "top\n\n<s>\nold\n</s>\n";
    const b = "TOP\n\n<s>\nnew\n</s>\n";
    const change = diff(a, { from: 5, to: 17 }, b, { from: 5, to: 17 }, "\n");
    let count = 0;
    change.iterChangedRanges(() => count++);
    expect(count).toBe(2);
    expect(change.apply(Text.of(a.split("\n"))).toString()).toBe(b);
  });

  it("does not split surrogate pairs", () => {
    expect(apply("x😀y", "x😃y", "\n")).toBe("x😃y");
  });
});

describe("toDocPos", () => {
  it("counts CRLF as one position", () => {
    expect(toDocPos("a\r\nb", 4, "\r\n")).toBe(3);
    expect(toDocPos("a\nb", 3, "\n")).toBe(3);
  });
});

describe("overlaps", () => {
  it("detects touching edits", () => {
    const base = "0123456789";
    const x = diff(base, null, "01X3456789", null, "\n");
    expect(overlaps(x, diff(base, null, "012345678Y", null, "\n"))).toBe(false);
    expect(overlaps(x, diff(base, null, "01Z3456789", null, "\n"))).toBe(true);
    expect(overlaps(x, diff(base, null, "012Z3456789", null, "\n"))).toBe(true);
  });
});
