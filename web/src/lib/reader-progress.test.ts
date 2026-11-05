import { describe, expect, it, vi } from "vitest";
import { createPageRecorder, QUIET_MS } from "./reader-progress";

/**
 * The rule that decides when a page turn becomes a stored fact.
 *
 * The page is written after it has been stable, never twice, and never lost when the reader closes; a
 * failed write is not retried in a loop. Every one of those is a decision this file has to be able to
 * change without the reader noticing, so they are asked here rather than through the application.
 */
describe("the reader's page recorder", () => {
  /** A recorder over fake timers, with every written page recorded in order. */
  function recorder(quietMs = QUIET_MS) {
    const written: number[] = [];
    const fail = { now: false };
    const write = vi.fn(async (page: number) => {
      if (fail.now) throw new Error("the store refused the page");
      written.push(page);
    });
    vi.useFakeTimers();
    return { record: createPageRecorder(write, { quietMs }), written, write, fail };
  }

  it("writes a page only after it has been on screen for the quiet period", async () => {
    const { record, written } = recorder(100);

    record.observe(4);
    await vi.advanceTimersByTimeAsync(99);
    expect(written).toEqual([]);

    await vi.advanceTimersByTimeAsync(1);
    expect(written).toEqual([4]);
    vi.useRealTimers();
  });

  it("writes the page the reader stopped on, not every page they turned through", async () => {
    const { record, written } = recorder(100);

    record.observe(1);
    await vi.advanceTimersByTimeAsync(40);
    record.observe(2);
    await vi.advanceTimersByTimeAsync(40);
    record.observe(3);
    await vi.advanceTimersByTimeAsync(40);

    expect(written).toEqual([]);
    await vi.advanceTimersByTimeAsync(100);
    expect(written).toEqual([3]);
    vi.useRealTimers();
  });

  it("does not write the same page twice", async () => {
    const { record, written } = recorder(10);

    record.observe(7);
    await vi.advanceTimersByTimeAsync(20);
    record.observe(7);
    await vi.advanceTimersByTimeAsync(20);

    expect(written).toEqual([7]);
    vi.useRealTimers();
  });

  it("writes the pending page when the reader closes, without waiting for the quiet period", async () => {
    // The last page of a reading session is exactly the one a resume needs, and it is the one the quiet
    // period would still be holding when the window goes away.
    const { record, written } = recorder(60_000);

    record.observe(12);
    await record.flush();

    expect(written).toEqual([12]);
    vi.useRealTimers();
  });

  it("writes nothing on a close when the page was already written", async () => {
    const { record, written } = recorder(10);

    record.observe(5);
    await vi.advanceTimersByTimeAsync(20);
    await record.flush();

    expect(written).toEqual([5]);
    vi.useRealTimers();
  });

  it("keeps the reader unbothered when a write fails, and tries the next page", async () => {
    const { record, written, fail } = recorder(10);

    fail.now = true;
    record.observe(2);
    await vi.advanceTimersByTimeAsync(20);

    fail.now = false;
    record.observe(3);
    await vi.advanceTimersByTimeAsync(20);

    // The page that failed stays unrecorded rather than being retried; the next one goes through.
    expect(written).toEqual([3]);
    vi.useRealTimers();
  });

  it("writes nothing after it has been stopped", async () => {
    const { record, written } = recorder(10);

    record.observe(9);
    record.stop();
    await vi.advanceTimersByTimeAsync(50);
    await record.flush();

    expect(written).toEqual([]);
    vi.useRealTimers();
  });
});
