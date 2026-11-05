/**
 * The byte-range transport pdf.js needs when the document is not behind an HTTP server.
 *
 * pdf.js requests ranges by itself only for `http(s)` URLs, where it can send a `Range` header. A local
 * file opened through Tauri has no such channel, so the loader takes a `PDFDataRangeTransport` instead and
 * asks it for byte ranges directly; pdf.js never opens the file. This module is that adapter: it extends
 * pdf.js's own `PDFDataRangeTransport` — which carries the document length, the listener pdf.js registers
 * through `transportReady()`, and the range requests themselves — and answers `requestDataRange(begin,
 * end)` by calling an injected reader.
 *
 * What stays out of here on purpose: no file access (the injected reader owns it), no IPC, no protocol
 * names, no UI. This decides only what pdf.js is allowed to be told.
 */
import { PDFDataRangeTransport } from "pdfjs-dist";

/**
 * Reads the half-open byte range `[begin, end)` of the document.
 *
 * `signal` fires when the document is torn down or the transport has failed, and it is a request to stop
 * *waiting*: this module cannot cancel file I/O, and saying otherwise would be a promise it cannot keep.
 * Whether the underlying read responds — an IPC round trip, a `File` slice, anything else — is the
 * reader's decision; a reader that ignores the signal simply gets its answer dropped here.
 */
export type RangeReader = (begin: number, end: number, signal: AbortSignal) => Promise<Uint8Array>;

/**
 * Hands over the one failure that ends this transport, so the reader can fail the document once.
 *
 * A range pdf.js never receives would otherwise leave its loader waiting with nothing on screen and no
 * reason why. Called at most once per transport, and never after `abort()`.
 */
export type RangeFailure = (error: unknown) => void;

/** Whether this transport can still read and deliver, has failed for good, or was cancelled by pdf.js. */
type Phase = "loading" | "failed" | "aborted";

export class ReaderTransport extends PDFDataRangeTransport {
  readonly #read: RangeReader;
  readonly #fail: RangeFailure;
  /** Cancellation for the reads in flight: fired by `abort()`, and by the first failure. */
  readonly #cancellation = new AbortController();
  /**
   * Loading at first, then terminal.
   *
   * Both terminal phases are one-way, because pdf.js never resurrects a document it cancelled or a load
   * that failed: a second range requested afterwards would be answered to nobody — pdf.js looks the range
   * up by its start offset and throws when it finds no reader — and a second failure report would show the
   * same broken document twice.
   */
  #phase: Phase = "loading";

  constructor(length: number, read: RangeReader, fail: RangeFailure) {
    // No initial data: pdf.js asks for what it needs, and a local file has no prologue to hand over. The
    // length is what makes range loading possible at all — without it pdf.js falls back to reading
    // everything.
    super(length, null);
    this.#read = read;
    this.#fail = fail;
  }

  override requestDataRange(begin: number, end: number): void {
    if (this.#phase !== "loading") return;
    void this.#load(begin, end);
  }

  override abort(): void {
    // pdf.js calls this once when it tears the document down. Repeat calls are harmless, and a call after a
    // failure still cancels the reads in flight — the document is over either way, and those reads are the
    // only thing left that can stop.
    if (this.#phase === "loading") this.#phase = "aborted";
    this.#cancellation.abort();
  }

  /**
   * Reads one range and hands it to pdf.js at the offset it asked for.
   *
   * Every failure — a rejected read, a synchronous throw, a chunk of the wrong size, a listener that
   * refuses it — goes through `#failOnce` instead of escaping: this promise is deliberately unobserved
   * (`void`ed above), so a throw leaving it would be an unhandled rejection that fails nothing and reaches
   * nobody. An answer that arrives for a transport that is no longer loading is dropped, not delivered.
   */
  async #load(begin: number, end: number): Promise<void> {
    let chunk: Uint8Array;
    try {
      chunk = await this.#read(begin, end, this.#cancellation.signal);
    } catch (error) {
      this.#failOnce(error);
      return;
    }

    if (this.#phase !== "loading") return;

    try {
      // pdf.js treats the chunk as exactly the bytes it asked for and places it at `begin`, so a shorter or
      // longer chunk would silently corrupt the document's offsets. Refusing it fails visibly instead.
      const expected = end - begin;
      if (chunk.byteLength !== expected) {
        throw new Error(`range ${begin}-${end} needs ${expected} bytes, the reader returned ${chunk.byteLength}`);
      }

      this.onDataRange(begin, chunk);
    } catch (error) {
      this.#failOnce(error);
    }
  }

  /** Reports the single failure that ends this transport, and stops the reads that are still in flight. */
  #failOnce(error: unknown): void {
    if (this.#phase !== "loading") return;
    // Settled before the callback runs: a callback that asks for another range must not start a read, and
    // an answer already on its way must not be delivered after the document has been reported broken.
    this.#phase = "failed";
    this.#cancellation.abort();

    try {
      this.#fail(error);
    } catch {
      // The callback is the last party that hears about this failure; when it throws there is nobody left to
      // report to, and rethrowing would only turn a reported failure into an unhandled rejection.
    }
  }
}
