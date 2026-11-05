import { describe, expect, it, vi } from "vitest";
import { ReaderTransport } from "./reader-transport";

/**
 * Node's own notice that a rejection reached nobody.
 *
 * `process` is untyped here because the repository's `tsconfig` carries no Node types, and this listener is
 * the only way to observe a rejection nothing awaited — exactly the failure this transport must not create.
 */
function watchUnhandledRejections(): { reasons: unknown[]; stop(): void } {
  const host = globalThis as unknown as {
    process: {
      on(event: "unhandledRejection", listener: (reason: unknown) => void): void;
      off(event: "unhandledRejection", listener: (reason: unknown) => void): void;
    };
  };
  const reasons: unknown[] = [];
  const listener = (reason: unknown) => reasons.push(reason);
  host.process.on("unhandledRejection", listener);
  return { reasons, stop: () => host.process.off("unhandledRejection", listener) };
}

/** Lets every settled promise run and Node report whatever nothing handled. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 20));
}

describe("reader byte-range transport", () => {
  it("requests only the range pdf.js asks for and delivers it at its offset", async () => {
    const read = vi.fn().mockResolvedValue(new Uint8Array([1, 2, 3]));
    const transport = new ReaderTransport(100, read, () => {});
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});
    transport.requestDataRange(10, 13);
    await vi.waitFor(() => expect(deliver).toHaveBeenCalledWith(10, new Uint8Array([1, 2, 3])));
    expect(read).toHaveBeenCalledWith(10, 13, expect.any(AbortSignal));
  });

  it("reports a failed range read rather than delivering partial data", async () => {
    const error = new Error("read failed");
    const report = vi.fn();
    const transport = new ReaderTransport(100, vi.fn().mockRejectedValue(error), report);
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});
    transport.requestDataRange(10, 13);
    await vi.waitFor(() => expect(report).toHaveBeenCalledWith(error));
    expect(deliver).not.toHaveBeenCalled();
  });

  it("reports a range answered with the wrong number of bytes instead of delivering it", async () => {
    const read = vi.fn().mockResolvedValue(new Uint8Array([1, 2]));
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});
    transport.requestDataRange(10, 13);
    await vi.waitFor(() => expect(report).toHaveBeenCalledWith(expect.any(Error)));
    expect(deliver).not.toHaveBeenCalled();
  });

  it("reports a reader that throws instead of rejecting", async () => {
    const error = new Error("open failed");
    const report = vi.fn();
    const transport = new ReaderTransport(
      100,
      vi.fn(() => {
        throw error;
      }),
      report,
    );
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});
    transport.requestDataRange(10, 13);
    await vi.waitFor(() => expect(report).toHaveBeenCalledWith(error));
    expect(deliver).not.toHaveBeenCalled();
  });

  it("drops a range that arrives after the transport was aborted", async () => {
    let answer: (chunk: Uint8Array) => void = () => {};
    const read = vi.fn(
      () =>
        new Promise<Uint8Array>((resolve) => {
          answer = resolve;
        }),
    );
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});

    transport.requestDataRange(10, 13);
    transport.abort();
    answer(new Uint8Array([1, 2, 3]));

    await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(1));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(deliver).not.toHaveBeenCalled();
    expect(report).not.toHaveBeenCalled();
  });

  it("issues no read for a range requested after the transport was aborted", () => {
    const read = vi.fn().mockResolvedValue(new Uint8Array([1, 2, 3]));
    const transport = new ReaderTransport(100, read, () => {});

    transport.abort();
    transport.requestDataRange(10, 13);

    expect(read).not.toHaveBeenCalled();
  });

  it("delivers through the listener pdf.js registers before it asks", async () => {
    const transport = new ReaderTransport(7, async (begin, end) => new Uint8Array(end - begin).fill(begin), () => {});
    const listener = vi.fn();
    transport.transportReady(listener);

    transport.requestDataRange(2, 5);

    await vi.waitFor(() =>
      expect(listener).toHaveBeenCalledWith({ type: "range", begin: 2, chunk: new Uint8Array([2, 2, 2]) }),
    );
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("reports a listener that refuses the delivered range instead of failing silently", async () => {
    const error = new Error("rejected range");
    const report = vi.fn();
    const transport = new ReaderTransport(7, async (begin, end) => new Uint8Array(end - begin), report);
    transport.transportReady(() => {
      throw error;
    });

    transport.requestDataRange(2, 5);

    await vi.waitFor(() => expect(report).toHaveBeenCalledWith(error));
  });
});

describe("reader transport after a failure", () => {
  /** Two range reads whose answers this test decides, so ordering is never left to timing. */
  function pendingReads() {
    const answers: Array<{ resolve(chunk: Uint8Array): void; reject(error: unknown): void }> = [];
    const signals: AbortSignal[] = [];
    const read = vi.fn(
      (_begin: number, _end: number, signal: AbortSignal) =>
        new Promise<Uint8Array>((resolve, reject) => {
          signals.push(signal);
          answers.push({ resolve, reject });
        }),
    );
    return { read, answers, signals };
  }

  it("reports only the first failure when two ranges fail", async () => {
    const { read, answers } = pendingReads();
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});
    const first = new Error("first range failed");
    const second = new Error("second range failed");

    transport.requestDataRange(0, 3);
    transport.requestDataRange(10, 13);
    answers[0].reject(first);
    answers[1].reject(second);

    await vi.waitFor(() => expect(report).toHaveBeenCalledTimes(1));
    await settle();
    expect(report).toHaveBeenCalledWith(first);
    expect(report).toHaveBeenCalledTimes(1);
    expect(deliver).not.toHaveBeenCalled();
  });

  it("delivers nothing from a range that answers after another range failed", async () => {
    const { read, answers } = pendingReads();
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});

    transport.requestDataRange(0, 3);
    transport.requestDataRange(10, 13);
    answers[1].reject(new Error("read failed"));
    await vi.waitFor(() => expect(report).toHaveBeenCalledTimes(1));

    answers[0].resolve(new Uint8Array([1, 2, 3]));

    await settle();
    expect(deliver).not.toHaveBeenCalled();
    expect(report).toHaveBeenCalledTimes(1);
  });

  it("issues no read for a range requested after the failure", async () => {
    const { read, answers } = pendingReads();
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);

    transport.requestDataRange(0, 3);
    answers[0].reject(new Error("read failed"));
    await vi.waitFor(() => expect(report).toHaveBeenCalledTimes(1));

    transport.requestDataRange(10, 13);

    expect(read).toHaveBeenCalledTimes(1);
  });

  it("cancels the reads still in flight when one range fails", async () => {
    const { read, answers, signals } = pendingReads();
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);

    transport.requestDataRange(0, 3);
    transport.requestDataRange(10, 13);
    answers[1].reject(new Error("read failed"));

    await vi.waitFor(() => expect(report).toHaveBeenCalledTimes(1));
    expect(signals[0].aborted).toBe(true);
  });

  it("stays failed when pdf.js aborts after the failure", async () => {
    const { read, answers } = pendingReads();
    const report = vi.fn();
    const transport = new ReaderTransport(100, read, report);
    const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});

    transport.requestDataRange(0, 3);
    transport.requestDataRange(10, 13);
    answers[1].reject(new Error("read failed"));
    await vi.waitFor(() => expect(report).toHaveBeenCalledTimes(1));

    transport.abort();
    answers[0].resolve(new Uint8Array([1, 2, 3]));

    await settle();
    expect(deliver).not.toHaveBeenCalled();
    expect(report).toHaveBeenCalledTimes(1);
  });

  it("does not create an unhandled rejection when the failure callback throws", async () => {
    const watched = watchUnhandledRejections();
    try {
      const transport = new ReaderTransport(
        100,
        vi.fn().mockRejectedValue(new Error("read failed")),
        () => {
          throw new Error("reporting failed");
        },
      );
      const deliver = vi.spyOn(transport, "onDataRange").mockImplementation(() => {});

      transport.requestDataRange(10, 13);

      await settle();
      expect(watched.reasons).toEqual([]);
      expect(deliver).not.toHaveBeenCalled();
    } finally {
      watched.stop();
    }
  });
});

describe("reader transport cancellation", () => {
  it("hands the reader a signal that only abort cancels", async () => {
    const signals: AbortSignal[] = [];
    const read = vi.fn(
      (_begin: number, _end: number, signal: AbortSignal) =>
        new Promise<Uint8Array>(() => {
          signals.push(signal);
        }),
    );
    const transport = new ReaderTransport(100, read, () => {});

    transport.requestDataRange(10, 13);
    expect(signals[0].aborted).toBe(false);

    transport.abort();
    transport.abort();

    expect(signals[0].aborted).toBe(true);
  });
});
