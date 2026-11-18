// Whether the window may close, given the editor's unsaved edits.
//
// Every close tries to save. If that fails (a conflict, or the disk
// refused), the close is refused, and only a second close soon after it,
// a deliberate "discard my edits", goes ahead. A later successful save
// ends that grace period, so an old refusal never lets edits go silently.

/** How soon after a refused close a second close discards the edits. */
export const DISCARD_WINDOW_MS = 60_000;

export interface Flushable {
  flush(): Promise<boolean>;
}

export class CloseGuard {
  private refusedAt = 0;

  constructor(private readonly now: () => number = Date.now) {}

  /** True if the window may close. */
  async mayClose(editor: Flushable | null): Promise<boolean> {
    if (!editor) return true;
    let saved = false;
    try {
      saved = await editor.flush();
    } catch {
      // Treated as unsaved.
    }
    if (saved) {
      this.saved();
      return true;
    }
    if (this.refusedAt && this.now() - this.refusedAt < DISCARD_WINDOW_MS) return true;
    this.refusedAt = this.now();
    return false;
  }

  /** Edits reached disk: a later failure needs a fresh refusal. */
  saved() {
    this.refusedAt = 0;
  }
}
