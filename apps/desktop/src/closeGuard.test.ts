import { describe, expect, it } from "vitest";

import { CloseGuard, DISCARD_WINDOW_MS } from "./closeGuard";

const editor = (results: boolean[]) => ({
  calls: 0,
  async flush() {
    return results[this.calls++] ?? true;
  },
});

describe("CloseGuard", () => {
  it("closes once edits are saved", async () => {
    expect(await new CloseGuard().mayClose(editor([true]))).toBe(true);
    expect(await new CloseGuard().mayClose(null)).toBe(true);
  });

  it("refuses the first close with unsaved edits; a second one discards", async () => {
    const guard = new CloseGuard();
    const e = editor([false, false]);
    expect(await guard.mayClose(e)).toBe(false);
    expect(await guard.mayClose(e)).toBe(true);
    expect(e.calls).toBe(2); // the second close still tried to save
  });

  it("treats a throwing flush as unsaved", async () => {
    const guard = new CloseGuard();
    expect(await guard.mayClose({ flush: () => Promise.reject(new Error("x")) })).toBe(false);
  });

  it("forgets the refusal once a later save succeeds", async () => {
    const guard = new CloseGuard();
    const e = editor([false, true, false]);
    expect(await guard.mayClose(e)).toBe(false); // refused
    expect(await guard.mayClose(e)).toBe(true); // the problem was fixed: saved
    expect(await guard.mayClose(e)).toBe(false); // a new failure is refused again
  });

  it("forgets the refusal after a save elsewhere, e.g. leaving the Piff", async () => {
    const guard = new CloseGuard();
    expect(await guard.mayClose(editor([false]))).toBe(false);
    guard.saved();
    expect(await guard.mayClose(editor([false]))).toBe(false);
  });

  it("forgets the refusal after a while", async () => {
    let t = 1_000;
    const guard = new CloseGuard(() => t);
    expect(await guard.mayClose(editor([false]))).toBe(false);
    t += DISCARD_WINDOW_MS;
    expect(await guard.mayClose(editor([false]))).toBe(false);
  });
});
