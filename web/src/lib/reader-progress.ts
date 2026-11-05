/**
 * When the reader's page becomes a stored fact.
 *
 * A continuous reader turns tens of pages a minute, and every page turn is a candidate for
 * `progress.page`. Writing each one would be a database write per scroll tick on a document somebody is
 * reading, and the last write of a burst is the only one anybody can observe. So the page is written
 * after it has been stable for a moment, and never twice for the same page.
 *
 * What this module owns is that decision and nothing else: no store, no IPC, no timer policy of its own.
 * The clock is injected, so the rule is testable without waiting for it.
 */

/** What the reader tells this module, and what it asks for on the way out. */
export interface PageRecorder {
  /** A page is on screen now. */
  observe(page: number): void;
  /** Writes whatever is pending right now. Called when the reader closes, so the last page is not lost. */
  flush(): Promise<void>;
  /** Stops the pending write without writing it. */
  stop(): void;
}

/** The parts of a clock this module needs, injected so a test can drive them. */
export interface RecorderClock {
  setTimeout(handler: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

/** Writes one page as the document's progress. */
export type PageWriter = (page: number) => Promise<void>;

/** How long a page must stay on screen before it is worth a write. */
export const QUIET_MS = 1200;

/**
 * Builds the recorder.
 *
 * The page is zero-based, exactly as `progress.page` is: the reader's own numbering starts at one, so the
 * conversion happens where the number is read, not here — a recorder that guessed would send page 0 for
 * page 1 and nobody would notice until a resume opened one page early.
 *
 * A failed write is swallowed deliberately: `progress` is a convenience for the next open, and a reader
 * whose page could not be stored must not be interrupted, told, or retried into a queue that grows with
 * every page turn. The next successful write is the correction.
 */
export function createPageRecorder(
  write: PageWriter,
  options: { quietMs?: number; clock?: RecorderClock } = {},
): PageRecorder {
  const quietMs = options.quietMs ?? QUIET_MS;
  const clock: RecorderClock = options.clock ?? {
    setTimeout: (handler, ms) => globalThis.setTimeout(handler, ms),
    clearTimeout: (handle) => globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>),
  };

  /** The page waiting to be written, or `null` when nothing is pending. */
  let pending: number | null = null;
  /** The last page this recorder committed, so a re-observe of it is not a second write. */
  let written: number | null = null;
  let timer: unknown = null;
  let stopped = false;

  function cancel(): void {
    if (timer !== null) {
      clock.clearTimeout(timer);
      timer = null;
    }
  }

  async function commit(page: number): Promise<void> {
    if (written === page) return;
    written = page;
    try {
      await write(page);
    } catch {
      // The page stays unrecorded; the next page turn tries again. Nothing is retried in a loop, because
      // a document the store cannot write is not a reason to spend the reader's battery on it.
    }
  }

  return {
    observe(page: number): void {
      if (stopped || page === written) return;
      pending = page;
      cancel();
      timer = clock.setTimeout(() => {
        timer = null;
        const target = pending;
        pending = null;
        if (target !== null) void commit(target);
      }, quietMs);
    },

    async flush(): Promise<void> {
      cancel();
      const target = pending;
      pending = null;
      if (target !== null) await commit(target);
    },

    stop(): void {
      stopped = true;
      cancel();
      pending = null;
    },
  };
}
