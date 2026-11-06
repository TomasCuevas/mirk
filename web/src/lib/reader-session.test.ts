/**
 * The owner's rule, pinned: count time only while the reader is visible and focused, and pause after
 * twenty minutes without interaction (L4). Every test drives an injected clock, so the rule is checked
 * without waiting twenty minutes for it.
 *
 * The instants leave here in Unix seconds and the durations in milliseconds, because that is what the
 * store writes: `ReadingSession` carries seconds for `started_at`/`ended_at` and milliseconds for
 * `active_ms`, and `PageVisit` carries seconds for `observed_at` and milliseconds for `visible_ms`.
 */
import { describe, expect, it } from "vitest";
import { IDLE_PAUSE_MS, createReadingSession } from "./reader-session";

const SECOND = 1000;
const MINUTE = 60 * SECOND;

/** A session whose clock the test moves by hand. */
function a_session() {
  let now = 1_700_000_000 * SECOND;
  const session = createReadingSession({ now: () => now });
  return {
    session,
    /** Lets `ms` pass with the session exactly as it is. */
    advance(ms: number) {
      now += ms;
    },
    /** Lets `ms` pass and reports an interaction at the end of it, as a scroll would. */
    advanceWithInteraction(ms: number) {
      now += ms;
      session.interact();
    },
    now: () => now,
  };
}

describe("the reading session's clock", () => {
  it("counts time only while the reader is visible and focused", () => {
    const { session, advance } = a_session();
    session.setActive(true);

    advance(10 * SECOND);
    session.setActive(false);

    expect(session.finish().activeMs).toBe(10 * SECOND);
  });

  it("does not count the gap while the window is hidden or unfocused", () => {
    const { session, advance } = a_session();
    session.setActive(true);
    advance(5 * SECOND);

    session.setActive(false);
    advance(10 * MINUTE);
    session.setActive(true);

    advance(5 * SECOND);

    expect(session.finish().activeMs).toBe(10 * SECOND);
  });

  it("stops counting after twenty minutes without interaction", () => {
    const { session, advance } = a_session();
    session.setActive(true);

    advance(30 * MINUTE);

    expect(session.finish().activeMs).toBe(IDLE_PAUSE_MS);
  });

  it("pauses from the moment the idle limit passed, not from the next interaction", () => {
    const { session, advance } = a_session();
    session.setActive(true);

    advance(IDLE_PAUSE_MS + 5 * MINUTE);
    session.interact();
    advance(MINUTE);

    // The five minutes between the pause and the interaction are not reading; the minute after it is.
    expect(session.finish().activeMs).toBe(IDLE_PAUSE_MS + MINUTE);
  });

  it("accumulates across several pauses without losing or double counting an interval", () => {
    const { session, advance, advanceWithInteraction } = a_session();
    session.setActive(true);

    advance(3 * MINUTE);
    session.setActive(false);
    advance(2 * MINUTE);
    session.setActive(true);
    advance(4 * MINUTE);
    advanceWithInteraction(MINUTE);
    advance(2 * MINUTE);

    expect(session.finish().activeMs).toBe(10 * MINUTE);
  });

  it("records one visit per page and collapses the page already on screen", () => {
    const { session, advance } = a_session();
    session.setActive(true);

    session.page(0);
    advance(SECOND);
    session.page(0);
    advance(SECOND);
    session.page(1);
    advance(SECOND);
    session.page(2);

    const visits = session.finish().visits;
    expect(visits.map((visit) => visit.page)).toEqual([0, 1, 2]);
  });

  it("records a new visit when the reader comes back to a page", () => {
    const { session, advance } = a_session();
    session.setActive(true);

    session.page(0);
    advance(SECOND);
    session.page(1);
    advance(SECOND);
    session.page(0);

    expect(session.finish().visits.map((visit) => visit.page)).toEqual([0, 1, 0]);
  });

  it("attributes visible time to the page that was on screen during it", () => {
    const { session, advance } = a_session();
    session.setActive(true);

    session.page(0);
    advance(4 * SECOND);
    session.page(1);
    advance(6 * SECOND);

    const visits = session.finish().visits;
    expect(visits.map((visit) => visit.visibleMs)).toEqual([4 * SECOND, 6 * SECOND]);
    expect(visits.reduce((total, visit) => total + visit.visibleMs, 0)).toBe(10 * SECOND);
  });

  it("counts nothing for a page on screen while the window is hidden", () => {
    const { session, advance } = a_session();
    session.setActive(true);
    session.page(0);
    advance(SECOND);

    session.setActive(false);
    advance(MINUTE);
    session.setActive(true);
    advance(SECOND);

    expect(session.finish().visits[0]?.visibleMs).toBe(2 * SECOND);
  });

  it("reports seconds for instants and milliseconds for durations", () => {
    const { session, advance } = a_session();
    session.setActive(true);
    session.page(3);
    advance(4 * SECOND);

    const facts = session.finish();
    const started = Math.floor(facts.startedAt);
    expect(Number.isInteger(facts.startedAt)).toBe(true);
    expect(Number.isInteger(facts.endedAt)).toBe(true);
    expect(facts.endedAt - started).toBe(4);
    expect(facts.activeMs).toBe(4 * SECOND);
    expect(facts.visits[0]?.observedAt).toBe(facts.startedAt);
    expect(facts.visits[0]?.visibleMs).toBe(4 * SECOND);
  });

  it("never reports negative time when the clock is corrected backwards", () => {
    const { session, advance, advanceWithInteraction } = a_session();
    session.setActive(true);
    session.page(0);
    advanceWithInteraction(10 * SECOND);

    advance(-30 * SECOND);

    const facts = session.finish();
    expect(facts.activeMs).toBeGreaterThanOrEqual(0);
    expect(facts.activeMs).toBe(10 * SECOND);
    expect(facts.endedAt).toBeLessThan(facts.startedAt);
    for (const visit of facts.visits) {
      expect(visit.visibleMs).toBeGreaterThanOrEqual(0);
    }
  });

  it("records nothing for a page the reader never showed", () => {
    const { session, advance } = a_session();
    session.setActive(true);
    advance(5 * SECOND);

    const facts = session.finish();
    expect(facts.visits).toEqual([]);
    expect(facts.activeMs).toBe(5 * SECOND);
  });
});
