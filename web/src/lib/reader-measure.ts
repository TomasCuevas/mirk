/**
 * What is actually on the screen, for the run that answers R1 and R2.
 *
 * The previous attempt's proof of a working reader was a log line saying a page had rendered. That is a
 * self-report: it proves a function ran, not that a page reached a canvas. This reads the two things that
 * cannot be self-reported — the page view's own per-page state, which it publishes in the DOM as
 * `.page-loading` / `.page-failure` inside each `data-page` slot, and the pixels of the canvas that claims
 * to be drawn.
 *
 * It is a measurement, not a feature: nothing outside a measurement run (`MIRK_READER_MEASURE`) reads it,
 * and no screen depends on these numbers.
 */
import type { PDFDocumentProxy } from "pdfjs-dist";

/** One page slot, as the page view currently describes it. */
export interface SlotReport {
  /** The page's number, from the slot's `data-page`. */
  page: number;
  /** The canvas's backing-store size. Zero means no canvas was created for the slot yet. */
  width: number;
  height: number;
  /** `drawn`, `drawing`, `waiting`, or `failed`. */
  state: string;
  /** What the page view said when it failed. */
  failure?: string;
  /** Share of sampled pixels the canvas has written to. Only measured for a drawn slot. */
  opaque?: number;
  /** Share of sampled pixels that are not the page's own white: the ink, which proves a page was read. */
  ink?: number;
}

/** What the page view holds, sampled while a measurement run waits for it. */
export interface PaintReport {
  /** How long the page view was given before this answer. */
  waitedMs: number;
  /** How many canvases existed when this was taken. */
  canvases: number;
  /** Every page slot the view had, in order. */
  slots: SlotReport[];
}

/**
 * Reads one canvas: the share of sampled pixels that are opaque, and the share that carry ink.
 *
 * A 16×16 grid of 16×16 patches: about 6% of a full page, spread across the margins and the text block,
 * which is dense enough that a page with text on it cannot come back empty and cheap enough to repeat.
 */
function sampleCanvas(canvas: HTMLCanvasElement): { opaque: number; ink: number } | null {
  const { width, height } = canvas;
  if (width === 0 || height === 0) return null;
  const context = canvas.getContext("2d");
  if (context === null) return null;

  const patch = 16;
  const across = 16;
  let sampled = 0;
  let opaque = 0;
  let ink = 0;

  for (let row = 0; row < across; row += 1) {
    for (let column = 0; column < across; column += 1) {
      const x = Math.min(Math.max(0, width - patch), Math.floor((column * width) / across));
      const y = Math.min(Math.max(0, height - patch), Math.floor((row * height) / across));

      let data: Uint8ClampedArray;
      try {
        data = context.getImageData(x, y, patch, patch).data;
      } catch {
        // A canvas that refuses to be read (tainted, or being torn down) says nothing about what was
        // drawn; the poll looks again.
        return null;
      }

      for (let pixel = 0; pixel < patch * patch; pixel += 1) {
        const index = pixel * 4;
        sampled += 1;
        if (data[index + 3] < 8) continue;
        opaque += 1;
        // pdf.js paints the page white and its content over it, so "not near-white" is the content.
        if (data[index] < 240 || data[index + 1] < 240 || data[index + 2] < 240) ink += 1;
      }
    }
  }

  if (sampled === 0) return null;
  return { opaque: opaque / sampled, ink: ink / sampled };
}

/** The state one slot publishes about its page, read from the page view's own markup. */
function readSlot(slot: Element): SlotReport {
  const page = Number((slot as HTMLElement).dataset.page ?? "0");
  const canvas = slot.querySelector("canvas");
  const width = canvas?.width ?? 0;
  const height = canvas?.height ?? 0;

  const failure = slot.querySelector(".page-failure");
  if (failure !== null) {
    return {
      page,
      width,
      height,
      state: "failed",
      failure: (failure.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 240),
    };
  }

  const loading = slot.querySelector(".page-loading");
  if (loading !== null) {
    const text = (loading.textContent ?? "").trim();
    return { page, width, height, state: text.startsWith("Drawing") ? "drawing" : "waiting" };
  }

  const report: SlotReport = { page, width, height, state: "drawn" };
  if (canvas !== null) {
    const sample = sampleCanvas(canvas);
    if (sample !== null) Object.assign(report, sample);
  }
  return report;
}

/**
 * Waits for a page to be drawn — or to fail — up to `timeoutMs`, and reports every slot it saw.
 *
 * The wait is bounded on purpose: a reader that never draws must answer "nothing was drawn" rather than
 * hang the measurement. A failure is answered immediately, because waiting for a page that has already
 * said it cannot be drawn only makes the report slower, not truer.
 */
export async function samplePaint(root: HTMLElement, timeoutMs = 25_000, intervalMs = 500): Promise<PaintReport> {
  const started = performance.now();
  let report: PaintReport = { waitedMs: 0, canvases: 0, slots: [] };

  while (performance.now() - started < timeoutMs) {
    const slots = [...root.querySelectorAll<HTMLElement>("[data-page]")].map(readSlot);
    report = {
      waitedMs: Math.round(performance.now() - started),
      canvases: root.querySelectorAll("canvas").length,
      slots,
    };

    if (slots.some((slot) => slot.state === "failed")) return report;
    if (slots.some((slot) => slot.state === "drawn" && (slot.ink ?? 0) > 0)) return report;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }

  return report;
}

/** Where a stuck page gave up, when the page view never finished drawing it. */
export interface PageProbe {
  /** The page number `getPage` answered, when it did. */
  page?: number;
  /** How many drawing operations the page's content produced, when it produced any. */
  operators?: number;
  /** The stage that did not answer within its budget: `getPage` or `getOperatorList`. */
  stuckAt?: string;
  /** What the stage threw, when it threw instead of waiting. */
  error?: string;
}

/** The part of a pdf.js page this file needs: enough to parse a page without drawing it. */
interface ProbePage {
  pageNumber: number;
  getOperatorList(): Promise<{ fnArray: unknown[] }>;
}

/** The part of a pdf.js document this file needs. */
interface ProbeDocument {
  getPage(page: number): Promise<ProbePage>;
}

/** Settles `promise` or answers `null` after `ms`, recording which stage did not answer. */
async function within<T>(stage: string, promise: Promise<T>, ms: number, report: PageProbe): Promise<T | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => {
    timer = setTimeout(() => {
      report.stuckAt = stage;
      resolve(null);
    }, ms);
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Parses page one without drawing it, to separate "the content is unavailable" from "the paint is stuck".
 *
 * A page view stuck on `drawing` can mean two very different things: pdf.js could not obtain the page's
 * content (a font, a wasm module, a range that never came), or it obtained it and the canvas render never
 * settled. `getOperatorList` answers the first question on its own, because it parses the content and
 * needs no canvas at all — so the stage that stops answering names the cause.
 */
export async function probeFirstPage(document: ProbeDocument, ms = 12_000): Promise<PageProbe> {
  const report: PageProbe = {};
  try {
    const page = await within("getPage", document.getPage(1), ms, report);
    if (page === null) return report;
    report.page = page.pageNumber;

    const operators = await within("getOperatorList", page.getOperatorList(), ms, report);
    if (operators === null) return report;
    report.operators = operators.fnArray.length;
    return report;
  } catch (error) {
    report.error = String(error);
    return report;
  }
}

/** Collects the errors the webview reports, which pdf.js raises without anyone awaiting them. */
export function collectErrors(limit = 4): { entries: string[]; stop: () => void } {
  const entries: string[] = [];
  const push = (entry: string): void => {
    if (entries.length < limit) entries.push(entry.slice(0, 240));
  };
  const onError = (event: ErrorEvent): void => push(`${event.message} (${event.filename}:${event.lineno})`);
  const onRejection = (event: PromiseRejectionEvent): void => push(`rejection: ${String(event.reason)}`);

  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);
  return {
    entries,
    stop: () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    },
  };
}

/** Whether pdf.js itself can paint a page in this webview, measured outside the page view. */
export interface RenderProbe {
  /** Whether `page.render` settled within the budget. */
  rendered?: boolean;
  /** The stage that did not answer: `getPage` or `render`. */
  stuckAt?: string;
  /** What it threw, when it threw instead of waiting. */
  error?: string;
  /** The canvas pdf.js was given, and the share of it that carries ink. */
  width?: number;
  height?: number;
  ink?: number;
}

/**
 * Draws page one into a canvas of its own, with nothing from the reader involved.
 *
 * This is the bisection the stuck page view cannot make about itself. If pdf.js completes this render and
 * puts ink on the canvas, the webview, the worker and the page content are all fine and the fault is in
 * the reader's own render bookkeeping. If this hangs too, the reader was never the problem.
 */
export async function probeRenderOnly(pdf: PDFDocumentProxy, ms = 15_000): Promise<RenderProbe> {
  const marker: PageProbe = {};
  try {
    const page = await within("getPage", pdf.getPage(1), ms, marker);
    if (page === null) return { stuckAt: marker.stuckAt };

    const viewport = page.getViewport({ scale: 1 });
    const canvas = globalThis.document.createElement("canvas");
    canvas.width = Math.max(1, Math.floor(viewport.width));
    canvas.height = Math.max(1, Math.floor(viewport.height));

    const report: RenderProbe = { width: canvas.width, height: canvas.height };
    const task = page.render({ canvas, viewport });
    const settled = await within("render", task.promise, ms, marker);
    if (settled === null) {
      task.cancel();
      return { ...report, rendered: false, stuckAt: marker.stuckAt };
    }

    report.rendered = true;
    const sample = sampleCanvas(canvas);
    if (sample !== null) report.ink = sample.ink;
    return report;
  } catch (error) {
    return { error: String(error) };
  }
}

/** Whether the webview hands out animation frames, which is what pdf.js paints in chunks on. */
export interface FrameProbe {
  /** How many `requestAnimationFrame` callbacks arrived in the budget. */
  frames: number;
  /** Whether the document believes it is hidden — a hidden window gets no compositor frames. */
  hidden: boolean;
  /** The document's own `visibilityState`. */
  visibility: string;
  /** Whether the window reports focus. */
  focused: boolean;
  /** The window's inner size, in case a zero-sized window is the reason frames stop. */
  width: number;
  height: number;
}

/**
 * Counts animation frames for `ms`.
 *
 * pdf.js does not paint a page in one go: `InternalRenderTask` splits the drawing into slices and schedules
 * the next slice on `requestAnimationFrame`. A webview that never delivers a frame — a window that was
 * never shown, one that is occluded, or a session with no compositor — therefore parses every page and
 * draws none, with no error anywhere. That is indistinguishable from a bug in the reader unless somebody
 * counts frames, which is what this does.
 */
export async function probeFrames(ms = 1200): Promise<FrameProbe> {
  const { document: dom } = globalThis;
  let frames = 0;
  const started = performance.now();

  await new Promise<void>((resolve) => {
    // The timer is the safety net: if no frame ever arrives, the poll must still answer.
    const timer = setTimeout(resolve, ms + 400);
    const tick = (): void => {
      frames += 1;
      if (performance.now() - started >= ms) {
        clearTimeout(timer);
        resolve();
        return;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });

  return {
    frames,
    hidden: dom.hidden,
    visibility: dom.visibilityState,
    focused: dom.hasFocus(),
    width: globalThis.innerWidth,
    height: globalThis.innerHeight,
  };
}
