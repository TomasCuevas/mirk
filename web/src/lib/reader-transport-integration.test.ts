import { getDocument } from "pdfjs-dist";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { ReaderTransport } from "./reader-transport";

/**
 * pdf.js's real loader, driven by the adapter this repository ships.
 *
 * The unit suite pins what `ReaderTransport` decides on its own. This file asks the opposite question: does
 * pdf.js — the actual `getDocument({ range })` entry point, its worker and its parser — load a real document
 * through it, and does it ask for bounded byte ranges while doing so?
 *
 * The documents are checked-in scanner fixtures, not generated PDFs, so the bytes parsed here are ones
 * another suite already depends on. `spike/r12/public/pdf` is deliberately not used: those entries are
 * symlinks into a personal library.
 *
 * Nothing here is Tauri: no IPC, no application window, no rendering. This proves the client-side loading
 * path only.
 */
const FIXTURES: ReadonlyArray<{ file: string; pages: number }> = [
  // 1,207 bytes with a classic cross-reference table: larger than pdf.js's 1 KiB header probe, so the
  // loader cannot satisfy the document with a single read.
  { file: "with-info-metadata.pdf", pages: 4 },
  // 484 bytes with its page objects inside a compressed object stream and its cross-reference in a stream:
  // the whole file fits in one header probe, so this case pins that a single range request still goes
  // through the transport instead of an eager buffer.
  { file: "pages-in-object-stream.pdf", pages: 3 },
];

/**
 * The two file operations this test needs, and nothing else.
 *
 * The suite's `node` project runs on Node, but the repository keeps `types` empty so that web code is not
 * given Node globals. `tsc` resolves a `node:fs/promises` import through `@types/node` anyway; `svelte-check`
 * does not, and a test that fails one of the two typechecks is not shippable. The specifier is therefore
 * assembled at runtime, which keeps Node out of the web type surface, and the handle is described by hand.
 */
type FixtureHandle = {
  stat(): Promise<{ size: number }>;
  read(buffer: Uint8Array, offset: number, length: number, position: number): Promise<{ bytesRead: number }>;
  close(): Promise<void>;
};
const fixtureFs = (await import(/* @vite-ignore */ "node:" + "fs/promises")) as {
  open(target: URL): Promise<FixtureHandle>;
};

/**
 * Three intrinsics pdf.js 6.4.299 calls on paths this test reaches, which Node 22.20 does not have.
 *
 * `Promise.try` runs the first worker message, `Uint8Array.prototype.toHex` runs while fingerprinting the
 * document, and `Map.prototype.getOrInsertComputed` is used by the range stream that asks this transport for
 * bytes. All three are absent from this runtime, so without them the loader hangs or fails before any page
 * exists. `Math.sumPrecise` is also missing here, but no path these two fixtures reach calls it, so it is not
 * shimmed at all rather than approximated by a plain sum that is not what the method means.
 *
 * They are installed once in `beforeAll` and removed in `afterAll`, because prototypes and `Promise` are
 * shared with every other test in the worker — and `afterAll` runs after failing tests too. Only a property
 * the runtime does not already own is defined, and `restore` removes exactly those; a failure part-way
 * through installation restores what it had already added, so a broken install changes nothing either.
 *
 * What the shims implement is the proposal semantics only as far as these documents reach them, not a
 * conformance implementation: `Promise.try` here ignores the callback's `this`, `toHex` covers the byte case
 * only, and `getOrInsertComputed` does not guard against re-entrant writes. That the gap exists at all is a
 * real compatibility risk for any webview older than the proposals; this test records it, it cannot close it.
 */
interface InstalledShim {
  target: object;
  key: string;
}

function installPdfjsRuntimeShims(): { installed: InstalledShim[]; restore(): void } {
  const added: InstalledShim[] = [];

  const defineMissing = (target: object, key: string, descriptor: PropertyDescriptor) => {
    if (Object.getOwnPropertyDescriptor(target, key) !== undefined) return;
    Object.defineProperty(target, key, { configurable: true, writable: true, ...descriptor });
    added.push({ target, key });
  };

  const restore = () => {
    for (const { target, key } of added.splice(0)) Reflect.deleteProperty(target, key);
  };

  try {
    defineMissing(Promise, "try", {
      value: (callback: (...args: unknown[]) => unknown, ...args: unknown[]) =>
        new Promise((resolve) => {
          resolve(callback(...args));
        }),
    });

    defineMissing(Uint8Array.prototype, "toHex", {
      value(this: Uint8Array): string {
        return Array.from(this, (byte) => byte.toString(16).padStart(2, "0")).join("");
      },
    });

    defineMissing(Map.prototype, "getOrInsertComputed", {
      value(this: Map<unknown, unknown>, key: unknown, compute: (key: unknown) => unknown): unknown {
        if (!this.has(key)) this.set(key, compute(key));
        return this.get(key);
      },
    });
  } catch (error) {
    restore();
    throw error;
  }

  return { installed: [...added], restore };
}

let runtimeShims: { installed: InstalledShim[]; restore(): void } = { installed: [], restore: () => {} };

beforeAll(() => {
  runtimeShims = installPdfjsRuntimeShims();
});

afterAll(() => {
  const { installed, restore } = runtimeShims;
  runtimeShims = { installed: [], restore: () => {} };
  restore();
  // Checked, not assumed: a shim that outlived this file would change what every other test in the worker
  // sees, so a failed removal is reported here rather than left to whatever runs next.
  for (const { target, key } of installed) {
    expect(Object.getOwnPropertyDescriptor(target, key)).toBeUndefined();
  }
});

/**
 * pdf.js's default is 64 KiB, far more than either fixture: every request would span the whole file and
 * "range loading" would prove nothing. A small chunk makes pdf.js plan the document as many small chunks,
 * so a request that leaves bytes for later is visible in the recorded ranges.
 */
const RANGE_CHUNK_SIZE = 64;

/** How long a reported rejection is given to reach Node, and how long a "still pending" claim is bounded to. */
const OBSERVATION_WINDOW_MS = 20;

interface RangeRequest {
  begin: number;
  end: number;
}

/**
 * A reader over the fixture that only ever hands back the bytes pdf.js asks for.
 *
 * Nothing here reads the file up front: each call reads a slice of the already-open handle at `begin`, and
 * the transport is constructed with no initial data. The bounds check is the point of the test — a request
 * outside `[0, length)` means pdf.js was told a length it did not honour, and serving it silently would turn
 * that into a document with shifted offsets.
 */
function boundedReader(handle: FixtureHandle, length: number) {
  const requests: RangeRequest[] = [];
  const signals: AbortSignal[] = [];

  const read = async (begin: number, end: number, signal: AbortSignal): Promise<Uint8Array> => {
    if (!Number.isInteger(begin) || !Number.isInteger(end) || begin < 0 || begin >= end || end > length) {
      throw new Error(`range ${begin}-${end} is not inside [0, ${length})`);
    }

    requests.push({ begin, end });
    signals.push(signal);

    const chunk = new Uint8Array(end - begin);
    const { bytesRead } = await handle.read(chunk, 0, chunk.byteLength, begin);
    if (bytesRead !== chunk.byteLength) {
      throw new Error(`range ${begin}-${end} ended at ${begin + bytesRead}, before the requested end`);
    }
    return chunk;
  };

  return { read, requests, signals };
}

/** Node's notice that a rejection reached nobody — the failure mode `ReaderTransport` must not create. */
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

/** Lets settled promises run so Node reports whatever nothing handled, and bounds the pending observation. */
function observeSettledRejections(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, OBSERVATION_WINDOW_MS));
}

describe("pdf.js loading through the reader transport", () => {
  it.each(FIXTURES)("loads $file and its first page from bounded ranges", async ({ file, pages }) => {
    const fixture = new URL(`../../../crates/mirk-scan/tests/fixtures/${file}`, import.meta.url);
    const handle = await fixtureFs.open(fixture);
    const unhandled = watchUnhandledRejections();
    try {
      const { size } = await handle.stat();
      const reader = boundedReader(handle, size);
      const reported: unknown[] = [];
      const transport = new ReaderTransport(size, reader.read, (error) => reported.push(error));

      const task = getDocument({
        // Deliberately no `data`: the document reaches pdf.js only through the transport below, and the
        // length it is given is the fixture's own.
        range: transport,
        rangeChunkSize: RANGE_CHUNK_SIZE,
        verbosity: 0,
      });

      // Destroyed in `finally`, so a rejecting load or a failed assertion still tears the task down and the
      // assertion reads as itself instead of leaving a worker behind.
      try {
        const pdf = await task.promise;

        expect(pdf.numPages).toBe(pages);

        const page = await pdf.getPage(1);
        expect(page.pageNumber).toBe(1);
        // The first page came back with geometry, which only a fully parsed page dictionary can produce.
        const viewport = page.getViewport({ scale: 1 });
        expect(viewport.width).toBeGreaterThan(0);
        expect(viewport.height).toBeGreaterThan(0);
      } finally {
        await task.destroy();
      }

      expect(transport.length).toBe(size);
      expect(reported).toEqual([]);

      // The document arrived, so every byte of it was requested through this reader, inside the file.
      expect(reader.requests.length).toBeGreaterThan(0);
      for (const { begin, end } of reader.requests) {
        expect(begin).toBeGreaterThanOrEqual(0);
        expect(end).toBeGreaterThan(begin);
        expect(end).toBeLessThanOrEqual(size);
      }
      // A file larger than pdf.js's 1 KiB header probe must be read in more than one range: proof the bytes
      // were not handed over in one eager buffer (a single request for the whole file cannot be avoided for
      // the 484-byte fixture, which fits inside that probe).
      if (size > 1024) {
        expect(reader.requests.length).toBeGreaterThan(1);
        expect(reader.requests.every(({ begin, end }) => end - begin < size)).toBe(true);
      }

      // pdf.js tear-down reaches the transport, which cancels the signal it gave the reader.
      expect(reader.signals.length).toBe(reader.requests.length);
      expect(reader.signals.every((signal) => signal.aborted)).toBe(true);

      await observeSettledRejections();
      expect(unhandled.reasons).toEqual([]);
    } finally {
      unhandled.stop();
      await handle.close();
    }
  });

  it("reports a failed range read once and stops, and pdf.js tears the transport down", async () => {
    const fixture = new URL("../../../crates/mirk-scan/tests/fixtures/pages-in-object-stream.pdf", import.meta.url);
    const handle = await fixtureFs.open(fixture);
    const unhandled = watchUnhandledRejections();
    // Declared outside the `try` so `finally` can always reach it: a setup failure before the loading task
    // exists leaves the no-op in place rather than throwing a second, obscuring error.
    let destroy: () => Promise<void> = async () => {};
    try {
      const { size } = await handle.stat();
      const error = new Error("range read failed");
      const requested = vi.fn(async (_begin: number, _end: number, _signal: AbortSignal): Promise<Uint8Array> => {
        throw error;
      });
      const signals: AbortSignal[] = [];
      const read = (begin: number, end: number, signal: AbortSignal) => {
        signals.push(signal);
        return requested(begin, end, signal);
      };

      const reported = vi.fn();
      let failLoad: (reportedError: unknown) => void = () => {};
      const failureReported = new Promise<unknown>((resolveFailure) => {
        failLoad = (reportedError) => {
          reported(reportedError);
          resolveFailure(reportedError);
        };
      });

      const transport = new ReaderTransport(size, read, (reportedError) => failLoad(reportedError));
      const task = getDocument({
        range: transport,
        rangeChunkSize: RANGE_CHUNK_SIZE,
        verbosity: 0,
      });
      // One destruction, awaited here and again in `finally`: a failed expectation cannot leave the loading
      // task alive, and the second await is the same promise rather than a second call.
      let destruction: Promise<void> | undefined;
      destroy = () => (destruction ??= task.destroy());
      // Installed before any await, so pdf.js's own cancellation cannot be missed.
      const abort = vi.spyOn(transport, "abort");

      const reportedError = await failureReported;
      expect(reportedError).toBe(error);

      expect(reported).toHaveBeenCalledTimes(1);
      expect(requested).toHaveBeenCalledTimes(1);

      // The tear-down an application performs from the failure callback, made explicit so the cleanup it
      // triggers can be observed.
      await destroy();

      // pdf.js's cleanup reaches `abort()`, and the transport cancels the signal it gave the reader.
      expect(abort).toHaveBeenCalled();
      expect(task.destroyed).toBe(true);
      expect(signals[0].aborted).toBe(true);

      // Terminal, not merely idle: another range must not start a read against a document given up on.
      transport.requestDataRange(0, 1);
      await observeSettledRejections();
      expect(requested).toHaveBeenCalledTimes(1);

      // Bounded observation, not a permanence claim: within the twenty milliseconds this test waits after
      // the failure and the tear-down, `task.promise` has neither resolved nor rejected. pdf.js has no
      // failure channel on the range transport, which is why the callback wiring above is required rather
      // than optional, and why nothing here asserts a rejection on the application's behalf.
      const state = await Promise.race([
        task.promise.then(
          () => "settled",
          () => "settled",
        ),
        observeSettledRejections().then(() => "still pending"),
      ]);
      expect(state).toBe("still pending");

      expect(unhandled.reasons).toEqual([]);
    } finally {
      await destroy();
      unhandled.stop();
      await handle.close();
    }
  });
});
