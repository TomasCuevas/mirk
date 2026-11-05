import { describe, expect, it, vi } from "vitest";
import { createRangeReader, parseContentRange, readLength, type SourceStats } from "./reader-source";

/** A partial answer carrying `bytes` filled with `value`, as the asset protocol would send it. */
function partial(bytes: Uint8Array<ArrayBuffer>, contentRange: string): Response {
  return new Response(bytes, { status: 206, headers: { "content-range": contentRange } });
}

/** Collects the `Range` header of every request, and the URL it went to. */
function recordRequests(): { urls: string[]; ranges: string[] } {
  const urls: string[] = [];
  const ranges: string[] = [];
  return { urls, ranges };
}

describe("reader source: the asset protocol as pdf.js's range channel", () => {
  describe("a Content-Range answer", () => {
    it("reports the range it covers and the document's size", () => {
      expect(parseContentRange("bytes 0-0/142801554")).toEqual({ begin: 0, end: 0, total: 142801554 });
      expect(parseContentRange("bytes 1024-2047/2048")).toEqual({ begin: 1024, end: 2047, total: 2048 });
    });

    it("refuses everything that is not one satisfiable range of one document", () => {
      // Absent, unknown size, a multi-range answer, an empty range, a backwards range, a range past the
      // end, and a non-numeric one: each of these would otherwise become an offset pdf.js trusts.
      expect(parseContentRange(null)).toBeNull();
      expect(parseContentRange("bytes 0-0/*")).toBeNull();
      expect(parseContentRange("bytes 0-9/100, 20-29/100")).toBeNull();
      expect(parseContentRange("bytes 5-4/100")).toBeNull();
      expect(parseContentRange("bytes 0-100/100")).toBeNull();
      expect(parseContentRange("items 0-0/1")).toBeNull();
    });
  });

  describe("the length probe", () => {
    it("asks for the smallest range and returns the reported size", async () => {
      const { ranges } = recordRequests();
      const request = vi.fn(async (_url: string, init?: RequestInit) => {
        ranges.push((init?.headers as Record<string, string>).Range);
        return partial(new Uint8Array([0]), "bytes 0-0/142801554");
      });

      await expect(readLength("asset://localhost/book.pdf", request)).resolves.toBe(142801554);
      expect(ranges).toEqual(["bytes=0-0"]);
    });

    it("fails when the protocol serves the whole file instead of a range", async () => {
      const request = vi.fn(async () => new Response(new Uint8Array([1]), { status: 200 }));

      await expect(readLength("asset://localhost/book.pdf", request)).rejects.toThrow(
        /answered 200 instead of a partial response/,
      );
    });

    it("fails when the size is not reported rather than guessing one", async () => {
      const request = vi.fn(async () => new Response(new Uint8Array([1]), { status: 206 }));

      await expect(readLength("asset://localhost/book.pdf", request)).rejects.toThrow(
        /did not report the document's size/,
      );
    });
  });

  describe("a range read", () => {
    it("fills a range the protocol's 1 MB cap splits, at the offsets pdf.js asked for", async () => {
      const stats: SourceStats = { length: 4_000_000, requests: 0, bytes: 0 };
      const { ranges } = recordRequests();
      const cap = 1024 * 1024;

      const request = vi.fn(async (_url: string, init?: RequestInit) => {
        ranges.push((init?.headers as Record<string, string>).Range);
        return ranges.length === 1
          ? partial(new Uint8Array(cap).fill(1), "bytes 0-1048575/4000000")
          : partial(new Uint8Array(1_500_000 - cap).fill(2), "bytes 1048576-1499999/4000000");
      });

      const read = createRangeReader("asset://localhost/book.pdf", stats, request);
      const chunk = await read(0, 1_500_000, new AbortController().signal);

      expect(chunk.byteLength).toBe(1_500_000);
      expect(chunk[0]).toBe(1);
      expect(chunk[cap - 1]).toBe(1);
      // The seam: the byte after the first response must be the first byte of the second, or every
      // offset in the document shifts by the difference.
      expect(chunk[cap]).toBe(2);
      expect(chunk[1_499_999]).toBe(2);
      expect(ranges).toEqual(["bytes=0-1499999", `bytes=${cap}-1499999`]);
      expect(stats).toEqual({ length: 4_000_000, requests: 2, bytes: 1_500_000 });
    });

    it("keeps only the bytes pdf.js asked for when the answer is longer", async () => {
      const stats: SourceStats = { length: 1000, requests: 0, bytes: 0 };
      const request = vi.fn(async () => partial(new Uint8Array([7, 8, 9, 10]), "bytes 10-13/1000"));
      const read = createRangeReader("asset://localhost/book.pdf", stats, request);

      const chunk = await read(10, 12, new AbortController().signal);
      expect(Array.from(chunk)).toEqual([7, 8]);
      expect(stats.bytes).toBe(2);
    });

    it("fails on a non-partial answer instead of returning short data", async () => {
      const stats: SourceStats = { length: 1000, requests: 0, bytes: 0 };
      const request = vi.fn(async () => new Response(new Uint8Array(4), { status: 200 }));
      const read = createRangeReader("asset://localhost/book.pdf", stats, request);

      await expect(read(0, 4, new AbortController().signal)).rejects.toThrow(
        /answered 200 instead of a partial response/,
      );
    });

    it("fails on an empty answer instead of looping for it", async () => {
      const stats: SourceStats = { length: 1000, requests: 0, bytes: 0 };
      const request = vi.fn(async () => partial(new Uint8Array(0), "bytes 0-3/1000"));
      const read = createRangeReader("asset://localhost/book.pdf", stats, request);

      await expect(read(0, 4, new AbortController().signal)).rejects.toThrow(/answered no bytes/);
      expect(stats.requests).toBe(1);
    });

    it("asks for nothing when the range is empty", async () => {
      const stats: SourceStats = { length: 1000, requests: 0, bytes: 0 };
      const request = vi.fn();
      const read = createRangeReader("asset://localhost/book.pdf", stats, request);

      const chunk = await read(10, 10, new AbortController().signal);
      expect(chunk.byteLength).toBe(0);
      expect(request).not.toHaveBeenCalled();
    });
  });
});
