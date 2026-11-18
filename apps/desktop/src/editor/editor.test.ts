// The editor against a fake backend. The fake's writer only mimics
// nodir-core closely enough to exercise the editor; the real writer is
// tested in Rust.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PiffDoc, SaveResult } from "../api";
import { sectionField } from "./section";

const START = "<!-- nodir:children:start -->";
const END = "<!-- nodir:children:end -->";
const SECTION = `${START}\n- [a/](a/PIFF.md)\n${END}`;

const disk = { text: null as string | null };

function sectionOf(text: string) {
  const from = text.lastIndexOf(START);
  const end = text.indexOf(END, from);
  return from === -1 || end === -1 ? null : { from, to: end + END.length };
}

function docOf(text: string | null): PiffDoc {
  return {
    exists: text !== null,
    text: text ?? "",
    section: text === null ? null : sectionOf(text),
    childrenHtml: "<ul><li>a/</li></ul>",
    lineEnding: text?.includes("\r\n") ? "\r\n" : "\n",
  };
}

function write(text: string) {
  const s = sectionOf(text);
  return s ? text.slice(0, s.from) + SECTION + text.slice(s.to) : `${text}\n\n${SECTION}\n`;
}

const saves: string[] = [];
let gate: Promise<void> | null = null;
/** Holds reads back; a read returns the disk as it was when it started. */
let readGate: Promise<void> | null = null;
/** Makes the next save fail, like a full disk. */
let failNextSave = false;

vi.mock("../api", () => ({
  api: {
    readPiff: async () => {
      const snapshot = disk.text;
      if (readGate) await readGate;
      return docOf(snapshot);
    },
    savePiff: async (_path: string, text: string, expected: string | null): Promise<SaveResult> => {
      if (gate) await gate;
      if (failNextSave) {
        failNextSave = false;
        throw new Error("disk full");
      }
      if (disk.text !== expected) return { kind: "conflict", doc: docOf(disk.text) };
      saves.push(text);
      disk.text = write(text);
      return { kind: "saved", doc: docOf(disk.text) };
    },
  },
}));

const { PiffEditor } = await import("./index");

const host = {
  followLink: vi.fn(),
  showError: vi.fn(),
  showConflict: vi.fn(),
  showStatus: vi.fn(),
};

let editor: InstanceType<typeof PiffEditor>;

function open(text: string | null) {
  disk.text = text;
  editor = new PiffEditor("/root", docOf(text), document.body, host);
  return editor;
}
const text = () => editor.view.state.sliceDoc();
const type = (pos: number, insert: string) =>
  editor.view.dispatch({ changes: { from: pos, insert }, userEvent: "input.type" });

beforeEach(() => {
  saves.length = 0;
  gate = null;
  readGate = null;
  failNextSave = false;
  vi.clearAllMocks();
});
afterEach(() => editor.destroy());

describe("PiffEditor", () => {
  it("creates PIFF.md lazily, only once something is typed", async () => {
    open(null);
    expect(text()).toBe("");
    await editor.flush();
    expect(disk.text).toBeNull();
    type(0, "# Hi");
    await editor.flush();
    expect(disk.text).toBe(`# Hi\n\n${SECTION}\n`);
    // The section written by the backend is now in the editor, read-only.
    expect(text()).toBe(disk.text);
    expect(editor.view.state.field(sectionField).range).toEqual({ from: 6, to: 6 + SECTION.length });
  });

  it("keeps typing done while a save is in flight", async () => {
    open(null);
    type(0, "one");
    let release!: () => void;
    gate = new Promise((r) => (release = r));
    const saving = editor.save();
    type(3, " two");
    release();
    await saving;
    expect(text()).toBe(`one two\n\n${SECTION}\n`);
    await editor.flush();
    expect(disk.text).toBe(`one two\n\n${SECTION}\n`);
  });

  it("never edits inside the children section", async () => {
    const initial = `Intro\n\n${SECTION}\n`;
    open(initial);
    const { from, to } = editor.view.state.field(sectionField).range!;
    editor.view.dispatch({ changes: { from: from + 5, to: from + 10, insert: "X" } });
    expect(text()).toBe(initial);
    // Select-all and type: user content is replaced, the section stays.
    editor.view.dispatch({ changes: { from: 0, to: text().length, insert: "New" } });
    expect(text()).toBe(`New\n${SECTION}`);
    expect(to).toBeGreaterThan(from);
  });

  it("keeps the markers on their own lines", () => {
    open(`${SECTION}\n`);
    type(0, "x");
    expect(text()).toBe(`x\n${SECTION}\n`);
    const end = editor.view.state.field(sectionField).range!.to;
    type(end, "y");
    expect(text()).toBe(`x\n${SECTION}\ny\n`);
  });

  it("reloads external edits when there are no unsaved edits", async () => {
    open(`Old\n\n${SECTION}\n`);
    disk.text = `New text\n\n${SECTION}\n`;
    await editor.checkDisk();
    expect(text()).toBe(disk.text);
    expect(host.showConflict).not.toHaveBeenCalled();
  });

  it("merges external edits that don't touch unsaved ones", async () => {
    open(`top\n\nbottom\n\n${SECTION}\n`);
    type(0, "my ");
    disk.text = `top\n\nbottom edited\n\n${SECTION}\n`;
    await editor.checkDisk();
    expect(text()).toBe(`my top\n\nbottom edited\n\n${SECTION}\n`);
    await editor.flush();
    expect(disk.text).toBe(`my top\n\nbottom edited\n\n${SECTION}\n`);
  });

  it("asks the user when external edits clash with unsaved ones", async () => {
    open(`line\n\n${SECTION}\n`);
    type(4, " mine");
    disk.text = `line theirs\n\n${SECTION}\n`;
    await editor.checkDisk();
    expect(host.showConflict).toHaveBeenLastCalledWith({ deleted: false });
    // No autosave while the conflict is open.
    await editor.flush();
    expect(disk.text).toBe(`line theirs\n\n${SECTION}\n`);

    await editor.keepMine();
    expect(host.showConflict).toHaveBeenLastCalledWith(null);
    expect(disk.text).toBe(`line mine\n\n${SECTION}\n`);
  });

  it("can drop unsaved edits for the disk version", async () => {
    open(`line\n\n${SECTION}\n`);
    type(4, " mine");
    disk.text = `line theirs\n\n${SECTION}\n`;
    await editor.checkDisk();
    editor.useDisk();
    expect(text()).toBe(disk.text);
    expect(editor.dirty).toBe(false);
  });

  it("detects a conflict at save time too", async () => {
    open(`line\n\n${SECTION}\n`);
    type(4, " mine");
    disk.text = `line theirs\n\n${SECTION}\n`; // watcher event not yet seen
    await editor.save();
    expect(host.showConflict).toHaveBeenLastCalledWith({ deleted: false });
    expect(disk.text).toBe(`line theirs\n\n${SECTION}\n`);
  });

  it("preserves CRLF line endings", async () => {
    const crlf = `# T\r\n\r\nbody\r\n\r\n${SECTION.replaceAll("\n", "\r\n")}\r\n`;
    open(crlf);
    type(3, "itle");
    editor.view.dispatch({ changes: { from: 13, insert: editor.view.state.lineBreak + "more" } });
    await editor.flush();
    expect(saves[0]).toBe(crlf.replace("# T", "# Title").replace("body", "body\r\nmore"));
  });

  describe("flush, before leaving a Piff", () => {
    it("also saves edits that had to be merged with a change on disk", async () => {
      open(`top\n\nbottom\n\n${SECTION}\n`);
      type(0, "my ");
      disk.text = `top\n\nbottom edited\n\n${SECTION}\n`; // watcher not yet heard
      expect(await editor.flush()).toBe(true);
      expect(editor.dirty).toBe(false);
      expect(disk.text).toBe(`my top\n\nbottom edited\n\n${SECTION}\n`);
    });

    it("reports a conflict instead of claiming success", async () => {
      open(`line\n\n${SECTION}\n`);
      type(4, " mine");
      disk.text = `line theirs\n\n${SECTION}\n`;
      expect(await editor.flush()).toBe(false);
      expect(editor.hasConflict).toBe(true);
      expect(text()).toContain("line mine");
    });

    it("reports a failed save and keeps the edits", async () => {
      open(`line\n\n${SECTION}\n`);
      type(4, " mine");
      failNextSave = true;
      expect(await editor.flush()).toBe(false);
      expect(editor.dirty).toBe(true);
      expect(host.showError).toHaveBeenCalled();
      // The next attempt succeeds.
      expect(await editor.flush()).toBe(true);
      expect(disk.text).toBe(`line mine\n\n${SECTION}\n`);
    });
  });

  it("ignores a disk read that started before a save", async () => {
    open(`line\n\n${SECTION}\n`);
    let release!: () => void;
    readGate = new Promise((r) => (release = r));
    const checking = editor.checkDisk(); // reads the pre-save text
    type(4, " saved");
    await editor.save();
    expect(disk.text).toBe(`line saved\n\n${SECTION}\n`);
    readGate = null;
    release();
    await checking;
    expect(text()).toBe(disk.text);
    expect(editor.dirty).toBe(false);
    expect(host.showConflict).not.toHaveBeenCalled();
  });
});
