/**
 * Where the reader's bytes come from: Tauri's own asset protocol, one byte range at a time.
 *
 * pdf.js only uses `Range` requests for a URL whose scheme is **http or https** — its fetch stream tests
 * `/https?:/` against the parsed protocol (`isValidFetchUrl`, `pdf.mjs`) — and on Linux a Tauri asset URL
 * is `asset://localhost/...`. So the document is never handed to the loader as a URL. It is handed a
 * `PDFDataRangeTransport` (see `./reader-transport`) whose reader is `fetch` against that same asset URL
 * with a `Range` header. The protocol does answer it: `206 PARTIAL_CONTENT`, `Accept-Ranges: bytes`,
 * `Content-Range`, and `content-range` exposed through CORS for precisely this case (read in
 * `tauri-2.12.3/src/protocol/asset.rs`).
 *
 * Two facts about that protocol shape this module:
 *
 * - One response is capped at **1 MB** and a longer request is truncated silently, so the
 *   `Content-Range` is read back and a range is filled in as many requests as it takes. pdf.js asks for
 *   64 KB at a time, which is one request in practice; the loop is what keeps the cap from becoming a
 *   silent corruption of every offset after it.
 * - The document's length is not known until it is asked for, and pdf.js cannot use ranges without it,
 *   so a single `bytes=0-0` probe resolves it before the loader exists.
 *
 * What stays out of here is a threat model. This reads a file the reader added to their own library and
 * that the operating system already lets this process read; the rule that matters is that a short or
 * truncated answer must fail visibly instead of shifting the document underneath pdf.js.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { GlobalWorkerOptions, getDocument, type PDFDocumentProxy } from "pdfjs-dist";
import workerSource from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { ReaderTransport } from "./reader-transport";

/** The same call shape as `fetch`, so a test can answer without a network and the app passes its own. */
export type Fetcher = (url: string, init?: RequestInit) => Promise<Response>;

/** What the document has cost so far: the size the protocol reported, and what reading it took. */
export interface SourceStats {
  /** The document's size in bytes, as `Content-Range` reported it. */
  length: number;
  /** How many requests the ranges cost. One range is one request unless the 1 MB cap splits it. */
  requests: number;
  /** How many bytes those requests read. */
  bytes: number;
}

/** One `Content-Range` answer: the range it covers, and the document's total size. */
export interface ContentRange {
  begin: number;
  end: number;
  total: number;
}

/** `bytes <begin>-<end>/<total>`, which is the only form the asset protocol answers. */
const CONTENT_RANGE = /^bytes (\d+)-(\d+)\/(\d+)$/;

/**
 * Reads a `Content-Range` header, or `null` when it is absent or not a single satisfiable range.
 *
 * A value that does not parse is treated as "the size is unknown" rather than guessed at: every offset
 * pdf.js computes afterwards depends on this number, so a wrong answer is worse than a refusal.
 */
export function parseContentRange(value: string | null): ContentRange | null {
  if (value === null) return null;
  const match = CONTENT_RANGE.exec(value.trim());
  if (match === null) return null;

  const begin = Number(match[1]);
  const end = Number(match[2]);
  const total = Number(match[3]);
  if (!Number.isSafeInteger(begin) || !Number.isSafeInteger(end) || !Number.isSafeInteger(total)) return null;
  // A range that starts after the document, ends before it starts, or ends past it describes nothing
  // this module can use — a multi-range answer arrives in this shape too and is not one document.
  if (begin > end || end >= total) return null;
  return { begin, end, total };
}

/**
 * Asks the protocol for the document's size with the smallest range that reports it.
 *
 * `bytes=0-0` is the standard probe: it costs one byte and answers `Content-Range: bytes 0-0/<total>`.
 * A `200` here means the protocol served the whole file instead of a range, which is the one answer that
 * cannot be turned into range loading — failing now says so plainly instead of loading 142 MB to find out.
 */
export async function readLength(url: string, request: Fetcher, signal?: AbortSignal): Promise<number> {
  const response = await request(url, { headers: { Range: "bytes=0-0" }, signal });
  if (response.status !== 206) {
    throw new Error(`the asset protocol answered ${response.status} instead of a partial response for ${url}`);
  }

  const range = parseContentRange(response.headers.get("content-range"));
  // The probe's one byte is never used; reading it keeps the response from outliving the answer.
  await response.arrayBuffer();
  if (range === null) {
    throw new Error(`the asset protocol did not report the document's size for ${url}`);
  }
  return range.total;
}

/**
 * Builds the `RangeReader` the transport calls, counting what it costs into `stats`.
 *
 * A range is filled in as many requests as the protocol's 1 MB cap requires. Each pass must add bytes: a
 * request that answers nothing is a failure rather than a reason to loop, so a server that truncates to
 * zero cannot hang the loader.
 */
export function createRangeReader(url: string, stats: SourceStats, request: Fetcher) {
  return async (begin: number, end: number, signal: AbortSignal): Promise<Uint8Array> => {
    const wanted = end - begin;
    if (wanted <= 0) return new Uint8Array(0);

    const out = new Uint8Array(wanted);
    let filled = 0;
    while (filled < wanted) {
      const from = begin + filled;
      const to = end - 1;
      const response = await request(url, { headers: { Range: `bytes=${from}-${to}` }, signal });
      stats.requests += 1;
      if (response.status !== 206) {
        throw new Error(`range ${from}-${to} answered ${response.status} instead of a partial response`);
      }

      const chunk = new Uint8Array(await response.arrayBuffer());
      if (chunk.byteLength === 0) {
        throw new Error(`range ${from}-${to} answered no bytes`);
      }

      const take = Math.min(chunk.byteLength, wanted - filled);
      out.set(chunk.subarray(0, take), filled);
      filled += take;
      stats.bytes += take;
    }
    return out;
  };
}

/** A loaded document, the numbers reading it cost, and the way to let it go. */
export interface ReaderSource {
  /** The document pdf.js has already loaded: exactly what `ReaderPages` takes as a prop. */
  document: PDFDocumentProxy;
  stats: SourceStats;
  /** The one failure that ended the transport, when it failed after the document had started loading. */
  failed(): unknown | null;
  /** Releases the document and its worker. Safe to call more than once. */
  destroy(): void;
}

/**
 * Opens `path` for the reader: probes its length, builds the transport and resolves once pdf.js has a
 * document. `ReaderPages` never opens anything itself, so this is the only place a file is named.
 */
export async function openReaderDocument(
  path: string,
  request: Fetcher = (url, init) => fetch(url, init),
): Promise<ReaderSource> {
  // The worker is a build asset Vite emits; pdf.js needs its URL before any document is loaded.
  GlobalWorkerOptions.workerSrc = workerSource;

  const url = convertFileSrc(path);
  const stats: SourceStats = { length: 0, requests: 0, bytes: 0 };
  stats.length = await readLength(url, request);

  let failure: unknown | null = null;
  const transport = new ReaderTransport(stats.length, createRangeReader(url, stats, request), (error) => {
    failure = error;
  });

  // No `disableRange`: the transport IS the range channel. Nothing else is passed, because pdf.js reads
  // the document through the transport and a per-call option it does not document is a guess.
  const task = getDocument({ range: transport });
  try {
    const document = await task.promise;
    return {
      document,
      stats,
      failed: () => failure,
      // The loading task owns the worker and the transport; the proxy is only what it produced.
      destroy: () => {
        void task.destroy();
      },
    };
  } catch (error) {
    // A load that never resolved leaves the transport's listener registered and its worker alive.
    void task.destroy();
    throw error;
  }
}
