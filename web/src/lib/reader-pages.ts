/**
 * The continuous reader's decisions, with no webview in them.
 *
 * A page-at-a-time viewer is easy: acquire a page, draw it, forget it. A continuous one is a memory and
 * ordering problem — the document has more pages than may exist at once, the scroll position decides
 * which ones are worth drawing, a zoom invalidates everything already drawn, and a render that is no
 * longer wanted must not keep its canvas or its place in the queue. Those decisions are made here, in
 * plain functions and one small state machine, so they can be tested deterministically; the component
 * (`web/src/components/ReaderPages.svelte`) does nothing but apply them to canvas elements and pdf.js.
 *
 * What this module is not: it does not open a document, choose a file, talk to the desktop process or
 * know what a session is. It receives an already-loaded pdf.js document — through the `ReaderDocument`
 * shape below — and draws pages out of it. Production wiring has to wait for the document transport,
 * which is why the only consumer today is the debug fixture page.
 *
 * # The bounds, and why they are here
 *
 * - **Visibility:** every page the viewport touches exists as a canvas, always. `maxWindowPages` is the
 *   budget for the pages that only the *overscan* band touches, so a viewport showing more pages than the
 *   budget keeps all of them rather than leaving a hole where a page should be. That is a deliberate
 *   tradeoff: the canvas count is bounded by what the reader can actually see, and the cost is paid by
 *   the render queue, which draws a couple at a time and queues the rest.
 * - **Jobs:** at most two renders run at once, and a cancelled render keeps its slot until its own
 *   promise settles, so a fast scroll or a zoom can never run two renders on one canvas.
 * - **Canvas:** one page's buffer is capped at {@link MAX_CANVAS_PIXELS} device pixels, and the device
 *   pixel ratio it asks for is capped at {@link MAX_DEVICE_PIXEL_RATIO}; a zoomed page on a 4x screen
 *   cannot quietly ask for sixteen times the memory it needs.
 * - **Measurement:** at most {@link MAX_MEASURED_PAGES} page sizes are read up front. A page beyond the
 *   bound is NOT given an invented size: it is `null`, its box is a marked `estimated` placeholder, and
 *   the layout is corrected when the page is acquired — because a differently shaped page stretched to
 *   an estimate is a distorted page, not an approximate scrollbar. The reader is told how many pages were
 *   measured, so the approximation is visible as an approximation.
 *
 * Releasing matters as much as bounding: an evicted page has its canvas buffer zeroed by the component
 * and its pdf.js page resources cleaned up, so the memory goes back instead of being kept "in case". A
 * cancelled or failed render releases the same way, but only after its promise has actually settled —
 * never while the render might still be using the canvas or the page.
 */

/** A page's size in PDF points, before any zoom. */
export interface PageSize {
  readonly width: number;
  readonly height: number;
}

/** One page's place in the continuous stack, in CSS pixels. */
export interface PageBox {
  /** The page number, counted from one like the document's pages. */
  readonly page: number;
  /** The distance from the top of the stack to the page's top edge. */
  readonly top: number;
  /** The page's left edge, inside the widest page's column. */
  readonly left: number;
  readonly width: number;
  readonly height: number;
  /**
   * Whether this box stands in for a page whose size has not been read yet.
   *
   * An estimated box is a placeholder for scrolling, never a shape to draw a real page into: the component
   * corrects the layout as soon as it acquires the page, and it sizes the canvas from the page's own
   * viewport, so a differently shaped page is never stretched to the estimate.
   */
  readonly estimated: boolean;
}

/** The whole stack at one zoom. */
export interface PageLayout {
  readonly scale: number;
  readonly gap: number;
  /** The gap the stack keeps above its first page and below its last, in its own coordinates. */
  readonly padding: number;
  readonly boxes: readonly PageBox[];
  /** The scrollable height of the stack. */
  readonly height: number;
  /** The widest page's width: the column every page is centred in. */
  readonly width: number;
}

/** Which pages the current scroll position may keep drawn. */
export interface PageWindow {
  /** Index of the first page in the stack, zero based. */
  readonly start: number;
  /** One past the index of the last page: `boxes.slice(start, end)` is the window. */
  readonly end: number;
  /** The page numbers of the window, ascending. */
  readonly pages: readonly number[];
  /**
   * The pages the viewport itself touches, ascending.
   *
   * This is the set the window may never drop: a page the reader is looking at has to exist, whatever a
   * canvas budget would prefer. `pages` is this set plus the overscan pages the budget still allows, so
   * the two differ exactly where the window has grown around what is visible.
   */
  readonly visible: readonly number[];
}

/** What one page's canvas buffer may be, in device pixels. */
export interface CanvasBuffer {
  readonly width: number;
  readonly height: number;
  /**
   * The device pixels per CSS pixel the buffer was built for.
   *
   * It is the ratio pdf.js should be asked to draw at: a smaller one than the screen's when the pixel
   * budget forced it down, so the drawing and the buffer stay in step.
   */
  readonly ratio: number;
}

/** Where the reader is standing: a page and how far down it. */
export interface ScrollAnchor {
  readonly page: number;
  /** The fraction of the page above the scroll offset, from 0 (its top) to 1 (its bottom). */
  readonly ratio: number;
}

/** The pages' sizes: a measured size, or `null` for a page nothing has acquired yet. */
export interface PageSizes {
  readonly sizes: readonly (PageSize | null)[];
  /** How many leading pages were really measured. The rest are unknown, not estimated. */
  readonly measured: number;
}

/** The smallest and largest zoom the reader offers. */
export const MIN_SCALE = 0.5;
export const MAX_SCALE = 4;

/** One press of a zoom control. */
export const ZOOM_STEP = 1.25;

/** The gap between pages in CSS pixels, at every zoom: it separates pages, it is not part of them. */
export const PAGE_GAP = 24;

/**
 * The gap the stack keeps above its first page and below its last, in its own coordinates.
 *
 * It is not the scroller's padding on purpose. If the scrolling element had vertical padding, `scrollTop`
 * would start above the stack while the layout started at the stack, and every window and anchor decision
 * would be off by that padding — enough to omit a page entering the last pixels of the viewport. The
 * padding belongs to the laid-out stack, so both share one origin. It matches `--space-5` from
 * `web/src/styles/tokens.css`; the component passes it into `pageLayout` and adds no padding of its own.
 */
export const PAGE_STACK_PADDING = 20;

/** How many pages may exist as canvases at once. */
export const DEFAULT_WINDOW_PAGES = 4;

/** How far above and below the viewport a page is worth drawing, so a small scroll has something ready. */
export const DEFAULT_OVERSCAN = 600;

/** How many page sizes are read before the rest are estimated from the last one. */
export const MAX_MEASURED_PAGES = 512;

/** How many renders may run at once. */
export const MAX_CONCURRENT_RENDERS = 2;

/** The screen ratio a canvas may ask for. A 3x screen gets 2x, because the third is not visible. */
export const MAX_DEVICE_PIXEL_RATIO = 2;

/** The largest buffer one page may allocate: four million pixels, sixteen megabytes at four bytes each. */
export const MAX_CANVAS_PIXELS = 4_000_000;

/** The size a page is laid out at when nothing about the document has been measured yet (US Letter). */
export const DEFAULT_PAGE_SIZE: PageSize = { width: 612, height: 792 };

/** A decision this module refuses to make, because making it would draw the wrong thing. */
export class ReaderPagesError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ReaderPagesError";
  }
}

function assertPositiveFinite(value: number, what: string): void {
  if (!Number.isFinite(value) || value <= 0) {
    throw new ReaderPagesError(`${what} must be a positive finite number, received ${value}`);
  }
}

function assertWholeNumberAtLeast(value: number, minimum: number, what: string): void {
  if (!Number.isInteger(value) || value < minimum) {
    throw new ReaderPagesError(
      `${what} must be a whole number of at least ${minimum}, received ${value}`,
    );
  }
}

function assertScale(scale: number): void {
  assertPositiveFinite(scale, "the render scale");
}

/**
 * Lays the pages out top to bottom at one zoom.
 *
 * Every page is scaled by the same factor and separated by the same gap, so the stack's geometry is a
 * function of the document and the zoom — not of the window. A narrower page is centred in the widest
 * page's column, which is what keeps a mixed-size document from looking ragged.
 *
 * `padding` is part of the stack's own geometry rather than of the container's: the scroll offset and
 * these coordinates have to share one origin, or the page the reader sees in the last few pixels of the
 * viewport is not the page this layout thinks is there. The caller therefore gives the scrolling element
 * no vertical padding and lets the stack claim the breathing room itself.
 */
export function pageLayout(
  sizes: readonly (PageSize | null)[],
  options: { readonly scale: number; readonly gap?: number; readonly padding?: number },
): PageLayout {
  const scale = options.scale;
  const gap = options.gap ?? PAGE_GAP;
  const padding = options.padding ?? 0;
  assertPositiveFinite(scale, "the layout scale");
  if (!Number.isFinite(gap) || gap < 0) {
    throw new ReaderPagesError(`the gap must be a finite, non-negative number, received ${gap}`);
  }
  if (!Number.isFinite(padding) || padding < 0) {
    throw new ReaderPagesError(
      `the padding must be a finite, non-negative number, received ${padding}`,
    );
  }

  // A page with no measured size borrows the nearest measured one — the first known size for pages before
  // any measurement, the last known for pages after it. It is a placeholder for scrolling, and `estimated`
  // says so: nothing may draw a real page into it, and it is replaced the moment the page is acquired.
  const firstKnown = sizes.find((size) => size !== null) ?? DEFAULT_PAGE_SIZE;
  let previousKnown: PageSize = firstKnown;

  const resolved = sizes.map((size) => {
    if (size !== null) previousKnown = size;
    return { size: size ?? previousKnown, estimated: size === null };
  });
  const width = resolved.reduce(
    (widest, entry) => Math.max(widest, entry.size.width * scale),
    0,
  );

  const boxes: PageBox[] = [];
  let top = padding;
  resolved.forEach((entry, index) => {
    assertPositiveFinite(entry.size.width, `page ${index + 1}'s width`);
    assertPositiveFinite(entry.size.height, `page ${index + 1}'s height`);
    const boxWidth = entry.size.width * scale;
    const boxHeight = entry.size.height * scale;
    boxes.push({
      page: index + 1,
      top,
      left: (width - boxWidth) / 2,
      width: boxWidth,
      height: boxHeight,
      estimated: entry.estimated,
    });
    top += boxHeight + gap;
  });

  // The last page's trailing gap is not scrollable space, but the stack's bottom padding is.
  const height = boxes.length === 0 ? 0 : top - gap + padding;
  return { scale, gap, padding, boxes, height, width };
}

/**
 * Chooses the pages a scroll position may keep drawn.
 *
 * Two bands decide it. The **viewport band** is what the reader can actually see; every page it touches
 * is in the window, always. The **overscan band** is the viewport plus the overscan on each side; pages
 * that only it touches are added while `maxPages` still has room, nearest the viewport's centre first,
 * and the earlier page wins a tie so the same offset always names the same window.
 *
 * `maxPages` is therefore a budget for what lies *outside* the viewport, not a cap on visibility. If the
 * viewport touches more pages than the budget, they are all kept and the budget is exceeded: dropping one
 * would leave a hole where a page should be. The cost of that decision is paid by the render queue, which
 * starts at most a couple of draws at a time and queues the rest, not by hiding a page.
 *
 * When nothing intersects the viewport at all — an offset in a gap, a zero-height viewport — the nearest
 * page by centre distance is named instead, so the reader always has something to show.
 */
export function pageWindow(
  layout: PageLayout,
  options: {
    readonly scrollTop: number;
    readonly viewportHeight: number;
    readonly overscan: number;
    readonly maxPages: number;
  },
): PageWindow {
  const { scrollTop, viewportHeight, overscan, maxPages } = options;
  if (!Number.isFinite(scrollTop)) {
    throw new ReaderPagesError(`the scroll offset must be a finite number, received ${scrollTop}`);
  }
  if (!Number.isFinite(viewportHeight) || viewportHeight < 0) {
    throw new ReaderPagesError(
      `the viewport height must be a finite, non-negative number, received ${viewportHeight}`,
    );
  }
  if (!Number.isFinite(overscan) || overscan < 0) {
    throw new ReaderPagesError(
      `the overscan must be a finite, non-negative number, received ${overscan}`,
    );
  }
  assertWholeNumberAtLeast(maxPages, 1, "the page budget");

  const boxes = layout.boxes;
  if (boxes.length === 0) {
    return { start: 0, end: 0, pages: [], visible: [] };
  }

  // A viewport of zero height still has to name a page, so the viewport band is never empty.
  const viewportStart = scrollTop;
  const viewportEnd = scrollTop + Math.max(viewportHeight, 1);
  const overscanStart = viewportStart - overscan;
  const overscanEnd = viewportEnd + overscan;
  const centre = (viewportStart + viewportEnd) / 2;

  const touches = (box: PageBox, from: number, to: number): boolean =>
    box.top < to && box.top + box.height > from;

  let visibleFirst = -1;
  let visibleLast = -1;
  boxes.forEach((box, index) => {
    if (!touches(box, viewportStart, viewportEnd)) return;
    if (visibleFirst === -1) visibleFirst = index;
    visibleLast = index;
  });

  if (visibleFirst === -1) {
    let nearest = 0;
    let nearestDistance = Number.POSITIVE_INFINITY;
    boxes.forEach((box, index) => {
      const distance = Math.abs(box.top + box.height / 2 - centre);
      // Strictly nearer: the earlier page wins a tie, so the same offset always names the same page.
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearest = index;
      }
    });
    visibleFirst = nearest;
    visibleLast = nearest;
  }

  let first = visibleFirst;
  let last = visibleLast;

  // Spend what the budget has left on the nearest pages the overscan band can reach. The loop grows the
  // window by one page at a time, so the budget is respected exactly and visibility is never trimmed.
  while (last - first + 1 < maxPages) {
    const before = first - 1;
    const after = last + 1;
    const beforeFits = before >= 0 && touches(boxes[before], overscanStart, overscanEnd);
    const afterFits = after < boxes.length && touches(boxes[after], overscanStart, overscanEnd);
    if (!beforeFits && !afterFits) break;

    const distance = (index: number): number =>
      Math.abs(boxes[index].top + boxes[index].height / 2 - centre);
    if (beforeFits && (!afterFits || distance(before) <= distance(after))) {
      first = before;
    } else {
      last = after;
    }
  }

  return {
    start: first,
    end: last + 1,
    pages: boxes.slice(first, last + 1).map((box) => box.page),
    visible: boxes.slice(visibleFirst, visibleLast + 1).map((box) => box.page),
  };
}

/**
 * Clamps a zoom inside the offered range.
 *
 * A scale that is not a positive finite number is refused rather than clamped: `NaN` reaching the layout
 * would produce `NaN` offsets and a stack of zero height, which looks like an empty document rather than
 * like a bug.
 */
export function clampScale(scale: number): number {
  assertPositiveFinite(scale, "the zoom");
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));
}

/**
 * One zoom press, clamped at the bounds.
 *
 * The result is rounded to four decimals so stepping in and back out returns exactly to the scale it
 * started from: `1 / 1.25` is not `0.8` in binary floating point, and a reader at "80%" that steps out
 * and in again must not end at "100.00000000000001%".
 */
export function zoomScale(scale: number, direction: "in" | "out"): number {
  const current = clampScale(scale);
  const next = direction === "in" ? current * ZOOM_STEP : current / ZOOM_STEP;
  return clampScale(Math.round(next * 10_000) / 10_000);
}

/**
 * The buffer one page's canvas may have, in device pixels.
 *
 * Three bounds meet here: the screen's ratio, {@link MAX_DEVICE_PIXEL_RATIO}, and the page's pixel
 * budget. When the budget is what binds, the ratio is lowered — a page drawn at 1.4x is a page, a page
 * whose allocation failed is not — and the rounded-up dimensions may then be a pixel short of the ideal,
 * which is why the pixel bound is checked again before they are used.
 */
export function canvasBuffer(
  cssWidth: number,
  cssHeight: number,
  ratio: number = MAX_DEVICE_PIXEL_RATIO,
): CanvasBuffer {
  assertPositiveFinite(cssWidth, "the page's CSS width");
  assertPositiveFinite(cssHeight, "the page's CSS height");
  assertPositiveFinite(ratio, "the device pixel ratio");

  const wanted = Math.min(ratio, MAX_DEVICE_PIXEL_RATIO);
  const fitted = Math.min(wanted, Math.sqrt(MAX_CANVAS_PIXELS / (cssWidth * cssHeight)));

  const rounded = {
    width: Math.max(1, Math.ceil(cssWidth * fitted)),
    height: Math.max(1, Math.ceil(cssHeight * fitted)),
  };
  if (rounded.width * rounded.height <= MAX_CANVAS_PIXELS) {
    return { ...rounded, ratio: fitted };
  }

  // Rounding up pushed the buffer past the budget. Rounding down cannot: `fitted` is at most the ratio
  // whose exact area is the budget, and flooring both dimensions can only shrink it further.
  return {
    width: Math.max(1, Math.floor(cssWidth * fitted)),
    height: Math.max(1, Math.floor(cssHeight * fitted)),
    ratio: fitted,
  };
}

/**
 * Names the page under a scroll offset and how far down it that offset sits.
 *
 * Used to keep the reading position across a zoom: the layout changes completely, but the page and the
 * fraction of it under the reader's eye are the position, not the pixel offset that used to mean it.
 * `null` when there is no page to anchor to.
 */
export function anchorAt(layout: PageLayout, scrollTop: number): ScrollAnchor | null {
  if (!Number.isFinite(scrollTop)) {
    throw new ReaderPagesError(`the scroll offset must be a finite number, received ${scrollTop}`);
  }
  const boxes = layout.boxes;
  if (boxes.length === 0) return null;

  let chosen = boxes[0];
  let nearestDistance = Number.POSITIVE_INFINITY;
  for (const box of boxes) {
    if (box.top <= scrollTop && scrollTop < box.top + box.height) {
      chosen = box;
      nearestDistance = 0;
      break;
    }
    const distance =
      scrollTop < box.top ? box.top - scrollTop : scrollTop - (box.top + box.height);
    if (distance < nearestDistance) {
      nearestDistance = distance;
      chosen = box;
    }
  }

  const ratio = Math.min(1, Math.max(0, (scrollTop - chosen.top) / chosen.height));
  return { page: chosen.page, ratio };
}

/** Where a page and a fraction of it sits in a layout, or `null` when that page is not in it. */
export function offsetForAnchor(layout: PageLayout, anchor: ScrollAnchor): number | null {
  const box = layout.boxes.find((candidate) => candidate.page === anchor.page);
  if (box === undefined) return null;
  const ratio = Math.min(1, Math.max(0, anchor.ratio));
  return box.top + ratio * box.height;
}

/** The part of pdf.js this reader needs: a page's viewport, its render, and its cleanup. */
export interface ReaderViewport {
  readonly width: number;
  readonly height: number;
}

/**
 * What the reader passes to `render`.
 *
 * The canvas is optional and nullable only because pdf.js's own `RenderParameters` declares it that way
 * (`canvas: HTMLCanvasElement | null`); this reader always supplies a canvas, and a page that receives
 * none is refused by the component's draw port rather than drawn somewhere invisible. Declaring it as
 * pdf.js does is what lets `PDFDocumentProxy` be injected without a wrapper.
 */
export interface ReaderRenderOptions {
  readonly canvas?: HTMLCanvasElement | null;
  readonly viewport: ReaderViewport;
}

/** A render task, as the reader needs it: when it finishes, and how to stop wanting it. */
export interface ReaderRenderTask {
  readonly promise: Promise<void>;
  cancel(extraDelay?: number): void;
}

/** One page of an already-loaded document. */
export interface ReaderPage {
  getViewport(options: { readonly scale: number }): ReaderViewport;
  render(options: ReaderRenderOptions): ReaderRenderTask;
  /** Frees the resources the page allocated. pdf.js has it; a test fake need not. */
  cleanup?(): boolean | void;
}

/**
 * The injected document.
 *
 * Structural on purpose: `PDFDocumentProxy` satisfies it, so the debug page hands over the document it
 * already loaded, and a test hands over a fake with the same three behaviours. The reader never builds
 * a document, so it cannot name a file, a document id or a transport.
 */
export interface ReaderDocument {
  readonly numPages: number;
  getPage(page: number): Promise<ReaderPage>;
}

/**
 * Reads the size of one page. The caller owns the page object, so it also owns its cleanup.
 *
 * It is a port rather than a `ReaderDocument` so the component can route acquisition through the same
 * cached page proxies its renders use, and so a measurement pass can be stopped and the pages it already
 * acquired released without guessing which pages those are.
 */
export type PageSizeMeasure = (page: number) => Promise<PageSize>;

/** Thrown when a measurement pass was stopped before it finished; the sizes it did read are not used. */
export class MeasurementStopped extends ReaderPagesError {
  constructor(message = "the page measurement was stopped") {
    super(message);
    this.name = "MeasurementStopped";
  }
}

/**
 * Reads the leading pages' sizes, up to `maxPages`, and stops promptly when asked.
 *
 * The pages beyond the bound are returned as `null`, never as a copy of the last measured size: a copy
 * would give a differently shaped page the wrong aspect ratio and stretch it. The layout treats `null`
 * as an explicit placeholder and replaces it as soon as the page is acquired.
 *
 * `signal` is checked before every acquisition and again after each one, so an abort both stops asking
 * for the next page and discards a size that arrives late. Which pages were acquired is the caller's
 * business — it supplied the measure function — so it can release exactly those.
 */
export async function readPageSizes(
  measure: PageSizeMeasure,
  pageCount: number,
  options: { readonly maxPages?: number; readonly signal?: AbortSignal } = {},
): Promise<PageSizes> {
  const maxPages = options.maxPages ?? MAX_MEASURED_PAGES;
  assertWholeNumberAtLeast(maxPages, 1, "the measurement bound");
  if (!Number.isInteger(pageCount) || pageCount < 1) {
    throw new ReaderPagesError(`the document reports ${pageCount} pages, so it has none to lay out`);
  }

  const signal = options.signal;
  const stopIfAsked = (): void => {
    if (signal?.aborted === true) throw new MeasurementStopped();
  };

  const measuredCount = Math.min(pageCount, maxPages);
  const sizes: (PageSize | null)[] = [];
  for (let page = 1; page <= measuredCount; page += 1) {
    stopIfAsked();
    const size = await measure(page);
    stopIfAsked();
    if (
      !Number.isFinite(size.width) ||
      !Number.isFinite(size.height) ||
      size.width <= 0 ||
      size.height <= 0
    ) {
      throw new ReaderPagesError(
        `page ${page} reports a ${size.width}×${size.height} size, so it cannot be laid out`,
      );
    }
    sizes.push({ width: size.width, height: size.height });
  }
  for (let page = sizes.length + 1; page <= pageCount; page += 1) {
    sizes.push(null);
  }
  return { sizes, measured: measuredCount };
}

/**
 * One page's measured size put into a sizes list, as a new list.
 *
 * Used when a page is acquired for drawing: the layout's placeholder is replaced with the page's real
 * size, which is what keeps a differently shaped page from being drawn into the estimate. Refuses a page
 * that is not in the list instead of growing it silently.
 */
export function withPageSize(
  sizes: readonly (PageSize | null)[],
  page: number,
  size: PageSize,
): readonly (PageSize | null)[] {
  assertWholeNumberAtLeast(page, 1, "a page number");
  if (page > sizes.length) {
    throw new ReaderPagesError(
      `page ${page} is not one of the ${sizes.length} pages the layout knows`,
    );
  }
  assertPositiveFinite(size.width, `page ${page}'s width`);
  assertPositiveFinite(size.height, `page ${page}'s height`);
  const next = sizes.slice();
  next[page - 1] = { width: size.width, height: size.height };
  return next;
}

/** One render the component has to perform. `promise` settles when pdf.js has finished, or refuses. */
export interface PageDraw {
  readonly promise: Promise<void>;
  cancel(): void;
  /**
   * Frees exactly what THIS draw took: the canvas element it drew into and the page's pdf.js resources.
   *
   * It is a method on the draw, not a page-number call on the component, because a page can be evicted and
   * come back with a new canvas element — or be measured, re-entered and drawn again — and releasing "the
   * page's canvas" would then zero the element a newer render is using. The set calls it once, and only
   * after `promise` has settled.
   */
  release(): void;
}

/**
 * Everything a render set needs from the interface, injected so the ordering can be tested without a
 * canvas.
 *
 * `draw` starts one page at one scale into whatever canvas the component has registered for that page and
 * answers a handle that owns that canvas and that page. Freeing memory is the handle's job, so the set
 * never has to guess which canvas a page currently has. `changed` says the snapshot a component renders
 * from is stale.
 */
export interface PageRenderPorts {
  draw(page: number, scale: number): PageDraw;
  changed(): void;
}

/** What the component has to draw right now, as one immutable answer. */
export interface PageRenderSnapshot {
  readonly scale: number;
  /** The pages the window asked for, in the order it asked. */
  readonly window: readonly number[];
  /** The pages whose canvas holds a drawing made at `scale`. */
  readonly drawn: readonly number[];
  /** The pages with a render running. */
  readonly drawing: readonly number[];
  /** The pages that could not be drawn, with the reason, so the interface can say so. */
  readonly failed: readonly { readonly page: number; readonly reason: string }[];
}

interface RenderJob {
  readonly page: number;
  readonly scale: number;
  cancelled: boolean;
  readonly handle: PageDraw;
}

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The render set: what to draw, what to stop drawing, and what to let go of.
 *
 * `setView` is the whole input. It is idempotent — the same window at the same scale draws nothing twice
 * — and it is where eviction happens: a page that left the window loses its canvas, and a page drawn at
 * the old scale loses it too (the new scale needs a new bitmap, and keeping the old one while the new one
 * is drawn is exactly the memory a continuous reader cannot spare).
 *
 * # Cancellation is asynchronous, and so is the accounting
 *
 * pdf.js's `renderTask.cancel()` only asks a render to stop; its promise settles later. A set that
 * treated "cancelled" as "finished" would reuse the concurrency slot too early, start a second render on
 * a canvas the first is still writing to, and release a page's pdf.js resources while a live render still
 * needs them. So a cancelled job moves to `#settling` and keeps BOTH its slot and its page until its
 * promise actually settles; only then is the page released (its canvas buffer and pdf.js resources) and
 * the queue advanced. A page is never drawn twice at once for the same reason, whatever the window and
 * zoom churn does.
 *
 * Cancellation is not failure: a cancelled job that rejects is not recorded as a broken page. A real
 * failure is, and it is released too — a failed render leaves nothing worth keeping — and is retried only
 * when asked (`retry`) or when the scale changes, never on every scroll event.
 *
 * `dispose()` stops all future work and releases every drawn canvas immediately, but the jobs it cancels
 * stay in `#settling`: their canvases and page resources are released when they really stop, even though
 * the set reports no state and starts nothing after disposal. That is what keeps disposal from leaking
 * the renders that were in flight when the reader left.
 */
export class PageRenderSet {
  readonly #ports: PageRenderPorts;
  readonly #maxConcurrent: number;
  #scale = 1;
  #window: number[] = [];
  /** page → the draw whose canvas currently shows it, with the scale it was drawn at. */
  #drawn = new Map<number, { readonly scale: number; readonly handle: PageDraw }>();
  /** page → the render running for it, wanted at the current view. */
  #jobs = new Map<number, RenderJob>();
  /** page → the render that was cancelled and has not settled yet: it still occupies its page and slot. */
  #settling = new Map<number, RenderJob>();
  /** page → why it failed, and the scale it failed at: a zoom is a new size, so a new attempt. */
  #failures = new Map<number, { readonly reason: string; readonly scale: number }>();
  #disposed = false;

  constructor(ports: PageRenderPorts, options: { readonly maxConcurrent?: number } = {}) {
    const maxConcurrent = options.maxConcurrent ?? MAX_CONCURRENT_RENDERS;
    assertWholeNumberAtLeast(maxConcurrent, 1, "the render bound");
    this.#ports = ports;
    this.#maxConcurrent = maxConcurrent;
  }

  /**
   * Applies one window and one scale.
   *
   * The order matters: jobs are cancelled before canvases are released, so nothing is still drawing into
   * a buffer that is about to be zeroed, and the window is stored before the queue is filled, so a
   * cancelled job's late settlement cannot start work for a window that has already been replaced.
   */
  setView(pages: readonly number[], scale: number): void {
    assertScale(scale);
    if (this.#disposed) return;

    const wanted: number[] = [];
    for (const page of pages) {
      assertWholeNumberAtLeast(page, 1, "a page number");
      if (!wanted.includes(page)) wanted.push(page);
    }
    const wantedSet = new Set(wanted);

    for (const job of [...this.#jobs.values()]) {
      if (job.scale !== scale || !wantedSet.has(job.page)) this.#cancel(job);
    }
    for (const [page, drawn] of [...this.#drawn]) {
      if (drawn.scale !== scale || !wantedSet.has(page)) {
        this.#drawn.delete(page);
        this.#releaseHandle(drawn.handle);
      }
    }
    for (const [page, failure] of [...this.#failures]) {
      if (failure.scale !== scale || !wantedSet.has(page)) this.#failures.delete(page);
    }

    this.#scale = scale;
    this.#window = wanted;
    this.#progress();
    this.#ports.changed();
  }

  /** Tries one failed page again, if it is still in the window. */
  retry(page: number): void {
    if (this.#disposed) return;
    if (!this.#window.includes(page)) return;
    if (!this.#failures.delete(page)) return;
    this.#progress();
    this.#ports.changed();
  }

  /** The route is leaving: cancel the work, release every drawn canvas, and stop accepting views. */
  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    for (const job of [...this.#jobs.values()]) this.#cancel(job);
    for (const [page, drawn] of [...this.#drawn]) {
      this.#drawn.delete(page);
      this.#releaseHandle(drawn.handle);
    }
    this.#window = [];
    this.#failures.clear();
    this.#ports.changed();
  }

  /**
   * A fresh, sorted snapshot.
   *
   * `drawing` is the renders genuinely running: a page whose render was cancelled is not drawing any
   * more, it is waiting, and the interface says so instead of showing a progress claim that is over.
   *
   * The arrays are copies: a component that sorts or slices what it was handed must not be able to
   * change what the set believes about itself.
   */
  snapshot(): PageRenderSnapshot {
    return {
      scale: this.#scale,
      window: [...this.#window],
      drawn: [...this.#drawn.keys()].sort((left, right) => left - right),
      drawing: [...this.#jobs.keys()].sort((left, right) => left - right),
      failed: [...this.#failures.entries()]
        .map(([page, failure]) => ({ page, reason: failure.reason }))
        .sort((left, right) => left.page - right.page),
    };
  }

  /**
   * Whether the set is holding anything for a page: a drawn canvas, a running render, or a cancelled one
   * that has not stopped yet.
   *
   * The component asks this before cleaning up a page it only measured: a proxy a render is using must be
   * released by that render's own handle, not under it.
   */
  isInUse(page: number): boolean {
    return this.#drawn.has(page) || this.#busyPage(page);
  }

  /** Whether a render occupies that page: being drawn now, or still stopping after a cancellation. */
  #busyPage(page: number): boolean {
    return this.#jobs.has(page) || this.#settling.has(page);
  }

  /** How many slots are taken: the running renders plus the cancelled ones that have not stopped. */
  #inFlight(): number {
    return this.#jobs.size + this.#settling.size;
  }

  /**
   * Hands one draw's resources back, once.
   *
   * The handle is the identity: it owns the canvas element it drew into, so a detached-and-reentered slot
   * or a newer render cannot be released by an older draw. A port that throws while releasing is not
   * allowed to break the queue — the set has already stopped using that draw.
   */
  #releaseHandle(handle: PageDraw): void {
    try {
      handle.release();
    } catch {
      // The draw is already gone from the set's bookkeeping; a failed release must not stop the queue.
    }
  }

  #cancel(job: RenderJob): void {
    if (job.cancelled) return;
    job.cancelled = true;
    this.#jobs.delete(job.page);
    // The slot and the page stay taken: the render may still be writing to its canvas and using its page.
    this.#settling.set(job.page, job);
    try {
      job.handle.cancel();
    } catch {
      // The job keeps its slot until its promise settles. A port that cannot cancel cannot be trusted to
      // have stopped, so nothing may run in its place.
    }
  }

  /** Starts what the window wants and the bounds allow, in window order. */
  #progress(): void {
    if (this.#disposed) return;
    for (const page of this.#window) {
      if (this.#inFlight() >= this.#maxConcurrent) return;
      if (this.#drawn.get(page)?.scale === this.#scale) continue;
      if (this.#busyPage(page)) continue;
      if (this.#failures.has(page)) continue;
      this.#start(page);
    }
  }

  #start(page: number): void {
    const scale = this.#scale;
    let handle: PageDraw;
    try {
      handle = this.#ports.draw(page, scale);
    } catch (error) {
      this.#failures.set(page, { reason: reasonOf(error), scale });
      // There is no handle to release: a draw that throws before returning one never gave the set anything
      // to hand back, so the port is responsible for whatever it took before failing.
      return;
    }

    const job: RenderJob = { page, scale, cancelled: false, handle };
    this.#jobs.set(page, job);
    handle.promise.then(
      () => this.#settle(job, null),
      (error: unknown) => this.#settle(job, error),
    );
  }

  #settle(job: RenderJob, error: unknown): void {
    if (job.cancelled) {
      // The cancellation is only now real: the render has stopped, so its own canvas and page can go back.
      if (this.#settling.get(job.page) !== job) return;
      this.#settling.delete(job.page);
      this.#releaseHandle(job.handle);
      if (this.#disposed) return;
      this.#progress();
      this.#ports.changed();
      return;
    }

    // A job that is no longer the set's current job was superseded; its answer describes a page the
    // reader has already moved away from, and must change nothing.
    if (this.#jobs.get(job.page) !== job) return;
    this.#jobs.delete(job.page);

    if (error !== null) {
      this.#failures.set(job.page, { reason: reasonOf(error), scale: job.scale });
      // A failed render leaves nothing worth keeping on its canvas or in the page's resources.
      this.#releaseHandle(job.handle);
    } else {
      this.#drawn.set(job.page, { scale: job.scale, handle: job.handle });
    }

    if (this.#disposed) return;
    this.#progress();
    this.#ports.changed();
  }
}

/**
 * Everything that belongs to ONE document, so a replaced document cannot touch the next one.
 *
 * A reader can be handed a different document while the previous one still has pages being acquired,
 * renders being cancelled and cleanups queued. Those completions arrive later, and the danger is that
 * they reach state that now belongs to somebody else: an old `getPage` answering a new document's render,
 * an old cancellation zeroing a new document's canvas, an old measurement cleaning a new document's page.
 * The cache is therefore per session, not per page number:
 *
 * - `#proxies` and `#measuredPages` belong to this document alone. A late old acquisition can only ever
 *   resolve, render and clean its own page objects; the new document has different ones for the same
 *   numbers.
 * - `#ownedPages` records the pages this document's draws have taken and not yet released, so measurement
 *   cleanup never pulls a page out from under a live render.
 * - `retire()` stops the session: it refuses new acquisitions and disposes its render set. A completion
 *   that was already in flight still releases ITS canvas and ITS page, because the draw handle captured
 *   them, and then touches nothing else.
 *
 * `canvas` is a port because the session never creates DOM nodes; the interface registers them per
 * generation, so a page's canvas from one document is a different element from the next document's.
 */
export interface PageSessionOptions {
  /** The document this session owns. */
  readonly document: ReaderDocument;
  /** The document's generation; the interface keys its canvases by it. */
  readonly generation: number;
  /** The canvas one page may be drawn into right now, if the interface has one. */
  canvas(page: number): HTMLCanvasElement | undefined;
  /** The screen's device pixel ratio, capped per canvas by `canvasBuffer`. */
  readonly pixelRatio: number;
  /** How many renders may run at once. */
  readonly maxConcurrent?: number;
  /** The session's render state changed. */
  changed(): void;
  /** A page's own size, read the moment it is acquired, so an estimated layout can be corrected. */
  measured?(page: number, size: PageSize): void;
}

/**
 * Who is using each page pdf.js has handed out, per DOCUMENT INSTANCE.
 *
 * pdf.js caches ONE page proxy per document and page: `PDFDocumentProxy.getPage` answers the same promise
 * to every caller. So two readers of the same document — or a replacement A → B → A that hands A back —
 * share those page objects, and cleanup cannot be a per-reader decision. It may only run when no owner of
 * that document and page is left; otherwise a reader could release resources under another reader's live
 * render. The registry is keyed by the document object and dies with it (WeakMap).
 *
 * Each page has ONE record, and each acquisition is one claim on it. The record remembers whether a cleanup
 * attempt is already waiting on the page's request, so several claims released before that request settles
 * produce one attempt rather than one cleanup each. A callback re-reads the record it was created for: a
 * newer owner's claim is never erased, and a page is never cleaned while an owner is left.
 */
interface PageRecord {
  /** How many per-acquisition claims this document and page currently have. */
  owners: number;
  /** Whether a cleanup attempt is already waiting on this page's request. */
  awaiting: boolean;
}

const pageRegistrations = new WeakMap<ReaderDocument, Map<number, PageRecord>>();

/** Registers one acquisition's claim on a page of one document. */
function acquirePageOwnership(document: ReaderDocument, page: number): void {
  let records = pageRegistrations.get(document);
  if (records === undefined) {
    records = new Map();
    pageRegistrations.set(document, records);
  }
  const record = records.get(page);
  if (record === undefined) records.set(page, { owners: 1, awaiting: false });
  else record.owners += 1;
}

/**
 * Gives up ONE acquisition's claim and, only when it was the last and no attempt is already waiting, asks
 * pdf.js to clean the page.
 *
 * Two kinds of staleness are handled here. The claim is identified by its acquisition, not by the page, so
 * a rejected request whose render handle is released later cannot decrement the same claim twice. And the
 * attempt re-reads the record it was created for: a newer claim keeps the page alive and its claim intact.
 *
 * `cleanup()` answers a boolean: pdf.js returns `false` while render tasks exist or the page's operator
 * list is incomplete. A refusal is NOT a cleanup. The record stays, so the NEXT owner's release (a real
 * trigger) asks again; nothing spins, and nothing claims the page was released.
 *
 * KNOWN LIMITATION — a refusal with no trigger is never retried. The only render-settlement event this
 * layer is given is the draw's own promise, and it settles BEFORE the draw's claim is released, so at that
 * moment the page still has an owner and a retry cannot run; the release that follows is already the
 * normal attempt. Once the last claim has been released and pdf.js refused, there is no event left to fire
 * a retry — inventing one would mean a timer or polling, which this reader refuses. The page therefore
 * keeps its (owners: 0) record until a later owner releases it or the document is collected, and the code
 * makes no claim that it will be cleaned.
 */
function releasePageOwnership(
  document: ReaderDocument,
  page: number,
  proxy: Promise<ReaderPage>,
): void {
  const records = pageRegistrations.get(document);
  const record = records?.get(page);
  if (records === undefined || record === undefined) return;

  record.owners = Math.max(0, record.owners - 1);
  if (record.owners > 0) return;
  // One attempt per page while one is pending: several claims released before the request settles must not
  // each clean the same resolved page.
  if (record.awaiting) return;
  record.awaiting = true;

  const stillCurrent = (): boolean => pageRegistrations.get(document)?.get(page) === record;

  proxy
    .then(
      (pageObject) => {
        record.awaiting = false;
        // A newer claim may hold the page now: cleaning would race its render and deleting the record
        // would erase its claim. The newer claim's own release is the next trigger.
        if (record.owners > 0 || !stillCurrent()) return;

        let cleaned: boolean | void = true;
        let threw = false;
        try {
          cleaned = pageObject.cleanup?.();
        } catch {
          // A cleanup that throws did NOT clean: the record stays so a later owner's release retries, and
          // the error is observed here instead of surfacing as an unhandled rejection.
          threw = true;
        }
        if (threw || cleaned === false) return;
        if (stillCurrent() && record.owners === 0) pageRegistrations.get(document)?.delete(page);
      },
      () => {
        record.awaiting = false;
        // The request produced no page, so there is nothing to clean and the failed claim is gone.
        if (stillCurrent() && record.owners === 0) pageRegistrations.get(document)?.delete(page);
      },
    )
    .catch(() => {
      // Whatever a callback, a cleanup or the bookkeeping threw, this attempt is over: ownership stays as
      // the record describes it and nothing escapes as an unhandled rejection.
    });
}

/**
 * One acquisition of one page: the request pdf.js answered, and whether this claim has been given up.
 *
 * Ownership is per acquisition, not per page, because one page can be acquired more than once (a failed
 * request is followed by another one) and each acquisition is one claim on the shared page.
 */
interface PageAcquisition {
  readonly promise: Promise<ReaderPage>;
  /** Set by whichever path releases this claim first; every later release is a no-op. */
  released: boolean;
}

/** One document's page ownership: its proxy cache, its measurements and its render set. */
export class PageSession {
  readonly document: ReaderDocument;
  readonly generation: number;
  readonly renders: PageRenderSet;

  readonly #canvas: (page: number) => HTMLCanvasElement | undefined;
  readonly #pixelRatio: number;
  readonly #measuredHook: ((page: number, size: PageSize) => void) | undefined;
  /** page → the acquisition this session holds for it; the claim is the acquisition, not the page. */
  readonly #proxies = new Map<number, PageAcquisition>();
  readonly #measuredPages = new Set<number>();
  /** Pages this document's draws have taken and not yet released. */
  readonly #ownedPages = new Set<number>();
  #retired = false;

  constructor(options: PageSessionOptions) {
    this.document = options.document;
    this.generation = options.generation;
    this.#canvas = options.canvas;
    this.#pixelRatio = options.pixelRatio;
    this.#measuredHook = options.measured;
    this.renders = new PageRenderSet(
      { draw: (page, scale) => this.#draw(page, scale), changed: options.changed },
      options.maxConcurrent === undefined ? {} : { maxConcurrent: options.maxConcurrent },
    );
  }

  /** Whether this document has been replaced; a retired session starts no new work. */
  get retired(): boolean {
    return this.#retired;
  }

  /**
   * The page proxy for this document, cached the way pdf.js caches it.
   *
   * Repeated calls answer the same promise, so a measurement and a render of the same page share one
   * acquisition. A retired session refuses synchronously: a late pass must not ask the document — which
   * may already be destroyed — for another page.
   */
  pageProxy(page: number): Promise<ReaderPage> {
    return this.#acquire(page).promise;
  }

  /**
   * This session's claim on a page, acquired once per residency.
   *
   * The claim is the acquisition object, so it is given up exactly once — whether the request fails, a
   * render handle releases it, or a measurement cleanup does. A failed request is released here; the draw
   * that was waiting on it will also try, and its release is a no-op because the claim is already gone.
   */
  #acquire(page: number): PageAcquisition {
    // Every acquisition goes through here, so a retired session can never ask the document for a page —
    // which may already be destroyed — whether the request comes from a render, a measurement or a draw.
    if (this.#retired) {
      throw new ReaderPagesError(
        `page ${page} belongs to a document that has been replaced and cannot be acquired`,
      );
    }
    let acquisition = this.#proxies.get(page);
    if (acquisition === undefined) {
      const promise = this.document.getPage(page);
      const created: PageAcquisition = { promise, released: false };
      this.#proxies.set(page, created);
      acquirePageOwnership(this.document, page);
      promise
        .catch(() => {
          this.#releaseAcquisition(page, created);
        })
        // The rejection is observed either way; this also contains a throw from the release itself so it
        // cannot reach the runtime as an unhandled rejection.
        .catch(() => {});
      acquisition = created;
    }
    return acquisition;
  }

  /**
   * One page's own size, for a layout that only had an estimate.
   *
   * The page is recorded BEFORE the retirement check, so a size that arrives after the document was
   * replaced is still something this session can clean up — its own page, never the new document's.
   */
  async measurePageSize(page: number): Promise<PageSize> {
    const acquisition = this.#acquire(page);
    // Recorded BEFORE the await: a claim still in flight when the pass stops is still this session's to
    // give up, instead of being held until the request happens to settle.
    this.#measuredPages.add(page);
    const proxy = await acquisition.promise;
    if (this.#retired) {
      throw new ReaderPagesError(`page ${page} was measured for a document that has been replaced`);
    }
    const viewport = proxy.getViewport({ scale: 1 });
    return { width: viewport.width, height: viewport.height };
  }

  /**
   * Gives back the pages a stopped measurement pass acquired.
   *
   * A page a draw is holding is skipped: that draw's release owns it, and cleaning it here could pull the
   * proxy out from under a live render. Only this document's proxies are ever touched.
   */
  releaseMeasuredPages(): void {
    for (const page of [...this.#measuredPages]) {
      this.#measuredPages.delete(page);
      if (this.#ownedPages.has(page)) continue;
      this.#cleanupProxy(page);
    }
  }

  /** Tries a failed page in this document again, if it is still in its window. */
  retry(page: number): void {
    if (this.#retired) return;
    this.renders.retry(page);
  }

  /**
   * Stops the session for good: no new acquisitions, its measured claims released, its renders disposed.
   *
   * Measurement claims are released here rather than left to the caller, so a session that is simply
   * dropped cannot hold a shared page alive forever. Pages a draw still owns are skipped: their handles
   * release them when they settle.
   */
  retire(): void {
    if (this.#retired) return;
    this.#retired = true;
    this.releaseMeasuredPages();
    this.renders.dispose();
  }

  /**
   * Gives up one acquisition's claim, exactly once.
   *
   * The flag is what stops a rejected request and the render handle that was waiting on it from each
   * decrementing the same claim — which would erase another reader's claim on the shared page.
   */
  #releaseAcquisition(page: number, acquisition: PageAcquisition): void {
    if (acquisition.released) return;
    acquisition.released = true;
    if (this.#proxies.get(page) === acquisition) this.#proxies.delete(page);
    releasePageOwnership(this.document, page, acquisition.promise);
  }

  /** Releases the claim this session currently holds for a page, if it still holds it. */
  #cleanupProxy(page: number): void {
    const acquisition = this.#proxies.get(page);
    if (acquisition === undefined) return;
    this.#releaseAcquisition(page, acquisition);
  }

  /**
   * Draws one page of this document into its own canvas.
   *
   * The returned handle owns the canvas element, the proxy promise and the render task it took, so its
   * release cannot reach any other document's canvas or page — and a draw that discovers it belongs to a
   * replaced document stops before pdf.js draws anything.
   */
  #draw(pageNumber: number, atScale: number): PageDraw {
    // A retired session starts no render at all: no canvas, no claim, no getPage.
    if (this.#retired) {
      throw new ReaderPagesError(`page ${pageNumber} belongs to a document that has been replaced`);
    }
    const canvas = this.#canvas(pageNumber);
    if (canvas === undefined) {
      throw new Error(`page ${pageNumber} has no canvas to draw into`);
    }

    this.#ownedPages.add(pageNumber);
    let cancelled = false;
    let released = false;
    let acquired: PageAcquisition | null = null;
    let task: { cancel(extraDelay?: number): void } | null = null;

    const promise = (async () => {
      const acquisition = this.#acquire(pageNumber);
      acquired = acquisition;
      const page = await acquisition.promise;
      // The document may have been replaced while the page was being acquired: stop before drawing.
      if (cancelled || this.#retired) {
        throw new Error(`page ${pageNumber} is no longer wanted`);
      }

      // The page's own size is read at 1:1 and the drawing size at the current zoom. They are different
      // facts: the correction must store the page's size, or the layout would scale an already scaled one.
      const native = page.getViewport({ scale: 1 });
      const css = page.getViewport({ scale: atScale });
      const buffer = canvasBuffer(css.width, css.height, this.#pixelRatio);
      // Setting either dimension reallocates the buffer, which is also how an old-scale bitmap is let go.
      canvas.width = buffer.width;
      canvas.height = buffer.height;
      canvas.style.width = `${css.width}px`;
      canvas.style.height = `${css.height}px`;
      this.#measuredHook?.(pageNumber, { width: native.width, height: native.height });

      const renderTask = page.render({
        canvas,
        viewport: page.getViewport({ scale: atScale * buffer.ratio }),
      });
      task = renderTask;
      // The job may have been cancelled while the page was being acquired.
      if (cancelled) renderTask.cancel();
      await renderTask.promise;
    })();
    // The render set observes this promise; this handler only keeps a cancellation from reaching the
    // window as an unhandled rejection.
    promise.catch(() => {});

    return {
      promise,
      cancel: () => {
        cancelled = true;
        task?.cancel();
      },
      release: () => {
        if (released) return;
        released = true;
        this.#ownedPages.delete(pageNumber);
        // This draw's own canvas element, captured when it started. Never "the page's canvas": a slot can
        // leave and come back, and another document may have a canvas for the same page number.
        canvas.width = 0;
        canvas.height = 0;

        if (acquired === null) return;
        this.#releaseAcquisition(pageNumber, acquired);
      },
    };
  }
}

/**
 * Guards a scroll offset that has to be written after the DOM has the stack's new height.
 *
 * Three things can make such a restore wrong: a newer restore (a second zoom, a size correction), the
 * reader scrolling in the meantime, and a document that was replaced while the restore was queued. Each
 * bumps the token — `begin` for a new restore, `invalidate` for a scroll or a replacement — and the
 * deferred write only happens if its token is still the current one.
 */
export class ScrollRestoreGuard {
  #token = 0;

  /** Registers a pending restore and answers the token it must still hold to be applied. */
  begin(): number {
    this.#token += 1;
    return this.#token;
  }

  /** Whether that restore may still run. */
  isCurrent(token: number): boolean {
    return token === this.#token;
  }

  /** Drops every pending restore: the reader scrolled, or the reader/document went away. */
  invalidate(): void {
    this.#token += 1;
  }
}
