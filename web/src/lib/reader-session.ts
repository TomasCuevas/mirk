/**
 * What a reading session is worth recording, and for how long (L4, R3b).
 *
 * The owner's rule: *count time only while the reader is visible and focused, and pause after twenty
 * minutes without interaction.* The two halves are separate on purpose. Visibility is a fact the window
 * reports, so a hidden or unfocused reader stops the clock the moment it happens. Idleness is a fact
 * nobody reports, so it is inferred from the last interaction: a reader who scrolled twice and walked
 * away is not still reading twenty-five minutes later, and the session must not claim they were.
 *
 * The pause begins where the limit passed, not where the reader came back. Somebody who returns after an
 * hour contributes the twenty minutes before the pause and nothing in between; the hour is not credited
 * to whoever happened to be at the keyboard next.
 *
 * What this module owns is the accounting: no store, no IPC and no DOM. The clock is injected, so the
 * twenty minutes are checked in a millisecond instead of waited out.
 *
 * The units are the store's: `startedAt`, `endedAt` and every visit's `observedAt` leave as Unix
 * seconds, because that is how `reading_sessions` and `page_visits` record an instant; `activeMs` and
 * every visit's `visibleMs` leave as milliseconds, because that is how the schema records a duration.
 */

/** Twenty minutes without interaction ends the accrual, however visible the window still is. */
export const IDLE_PAUSE_MS = 20 * 60 * 1000;

/** One page this session showed, with the time it spent on screen while the clock was running. */
export interface SessionVisit {
  /** Zero-based, exactly as `progress.page` and `page_visits.page` are. */
  page: number;
  /** Unix seconds. */
  observedAt: number;
  /** Visible-and-focused milliseconds this page spent on screen. */
  visibleMs: number;
}

/** The facts one session contributes, in the units the store writes. */
export interface SessionFacts {
  /** Unix seconds. */
  startedAt: number;
  /** Unix seconds. The clock can be corrected backwards, so this may be earlier than `startedAt`. */
  endedAt: number;
  /** Visible-and-focused milliseconds actually spent reading; never negative. */
  activeMs: number;
  visits: SessionVisit[];
}

export interface ReadingSession {
  /**
   * The reader did something: a scroll, a key, a pointer, a page turn. Only this lifts an idle pause.
   */
  interact(): void;
  /**
   * The window is visible and focused, or it is not. The session starts inactive: nothing is counted
   * until the reader's own window says it is on screen, so an application that is never shown records
   * no reading it did not do.
   */
  setActive(active: boolean): void;
  /** The page on screen now, zero-based. The same page again is not a second visit. */
  page(page: number): void;
  /** Closes the session and answers its facts. Calling it twice answers the same facts. */
  finish(): SessionFacts;
}

/** An interval as Unix seconds, the unit both tables store an instant in. */
function seconds(ms: number): number {
  return Math.floor(ms / 1000);
}

/**
 * Starts a session at the clock's now.
 *
 * A visit's `visibleMs` is the time that page spent on screen while the clock was running, so the sum of
 * a session's visits never exceeds its `activeMs`: time spent between pages, or with no page at all,
 * belongs to the session and to no page.
 */
export function createReadingSession(options: {
  /** The clock to read. Injected so the twenty minutes can be driven instead of waited out. */
  now: () => number;
  idlePauseMs?: number;
}): ReadingSession {
  const now = options.now;
  const idlePauseMs = options.idlePauseMs ?? IDLE_PAUSE_MS;

  const startedAtMs = now();
  /** The instant accrual has been integrated up to. Everything before it is already counted. */
  let integratedTo = startedAtMs;
  /** The last interaction, before which the idle limit is measured. */
  let lastInteraction = startedAtMs;
  let active = false;
  let accrued = 0;
  /** The visit the clock is currently attributing time to. */
  let current: { page: number; observedAtMs: number; visibleMs: number } | null = null;
  const visits: { page: number; observedAtMs: number; visibleMs: number }[] = [];
  let finished: SessionFacts | null = null;

  /**
   * Charges the interval between the last integration and `at`, capped at the instant the idle limit
   * passed. A clock corrected backwards contributes nothing rather than a negative interval.
   */
  function integrate(at: number): void {
    if (!active) {
      integratedTo = Math.max(integratedTo, at);
      return;
    }
    const idleBoundary = lastInteraction + idlePauseMs;
    const stop = Math.min(at, idleBoundary);
    if (stop > integratedTo) {
      const delta = stop - integratedTo;
      accrued += delta;
      if (current) current.visibleMs += delta;
      integratedTo = stop;
    }
  }

  return {
    interact(): void {
      if (finished) return;
      const at = now();
      integrate(at);
      lastInteraction = at;
      // Time between the idle pause and this interaction is not reading: resume at the interaction.
      integratedTo = Math.max(integratedTo, at);
    },

    setActive(value: boolean): void {
      if (finished) return;
      const at = now();
      integrate(at);
      active = value;
      // Coming back is a beginning, not a debt: the hidden interval is not counted towards anything.
      integratedTo = Math.max(integratedTo, at);
    },

    page(page: number): void {
      if (finished) return;
      const at = now();
      integrate(at);
      if (current?.page === page) return;
      current = { page, observedAtMs: at, visibleMs: 0 };
      visits.push(current);
    },

    finish(): SessionFacts {
      if (finished) return finished;
      const at = now();
      integrate(at);
      finished = {
        startedAt: seconds(startedAtMs),
        endedAt: seconds(at),
        activeMs: Math.max(0, accrued),
        visits: visits.map((visit) => ({
          page: visit.page,
          observedAt: seconds(visit.observedAtMs),
          visibleMs: Math.max(0, visit.visibleMs),
        })),
      };
      return finished;
    },
  };
}
