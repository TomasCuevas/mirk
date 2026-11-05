import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_PAGE_SIZE,
  MAX_CANVAS_PIXELS,
  MAX_DEVICE_PIXEL_RATIO,
  MAX_SCALE,
  MIN_SCALE,
  MeasurementStopped,
  PageRenderSet,
  PageSession,
  ReaderPagesError,
  ScrollRestoreGuard,
  anchorAt,
  canvasBuffer,
  clampScale,
  offsetForAnchor,
  pageLayout,
  pageWindow,
  readPageSizes,
  withPageSize,
  zoomScale,
  type PageDraw,
  type PageRenderPorts,
  type PageSize,
  type PageSizeMeasure,
  type PageSessionOptions,
  type ReaderDocument,
  type ReaderPage,
  type ScrollAnchor,
} from "./reader-pages";

/**
 * The continuous reader's decisions, without a webview.
 *
 * Everything here is the part of the reader that can be decided before a canvas exists: where the pages
 * sit at a given zoom, which of them a scroll position may have drawn, how much memory one canvas may
 * ask for, and which render jobs are started, cancelled and released. The component
 * (`web/src/components/ReaderPages.svelte`) only applies those decisions to canvases, so a mistake in
 * the arithmetic — a page drawn off screen, a job outliving its window, an evicted canvas kept alive —
 * is caught here, deterministically, without pdf.js or a browser.
 */

/** Four pages, all the same size: the shape a scanner produces and the fixture in the debug page. */
const SIZES = [
  { width: 300, height: 400 },
  { width: 300, height: 400 },
  { width: 300, height: 400 },
  { width: 300, height: 400 },
];

/** The default gap the component uses between pages, repeated here so the arithmetic is explicit. */
const GAP = 24;

/** Lets the microtask queue run, so a settled draw can start the next one before the assertion. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** Node's notice that a rejection reached nobody, observed the way the transport suites observe it. */
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

interface FakeDraw {
  readonly page: number;
  readonly scale: number;
  /** The identity of the canvas this draw went into. A release must name it, never just the page. */
  readonly canvas: number;
  cancelled: boolean;
  settled: boolean;
  released: boolean;
  /** Ends the draw: a plain resolution, or a rejection with that reason. Test-controlled on purpose. */
  settle(reason?: string): void;
}

interface FakeRelease {
  readonly page: number;
  /** The canvas of the draw that was released. */
  readonly canvas: number;
  /** Whether that draw had already settled when it was released. */
  readonly settled: boolean;
  /** Whether a render for that page was still running when it was released. */
  readonly whileDrawing: boolean;
}

/**
 * The ports, recorded instead of drawn, with DELAYED cancellation.
 *
 * `cancel()` only marks the draw; its promise settles when the test says so, which is what a real pdf.js
 * render task does — cancellation is cooperative, so the promise rejects on a later turn. That delay is
 * the point of this fake. A synchronously-settling cancellation cannot see the defects this suite is
 * about: a cancelled job that stops counting before it has stopped running lets the set start the next
 * render early, reuse a canvas a live render is still writing to, and release resources it still needs.
 *
 * Each draw gets its own canvas identity, and `detach()` models a slot leaving the document and coming
 * back with a new canvas element. Releasing through the page number instead of the draw's own handle would
 * then zero the new element — the defect this fake can see and a page-keyed release cannot.
 */
function fakePorts(): {
  ports: PageRenderPorts;
  draws: FakeDraw[];
  releases: FakeRelease[];
  /** Pages that were asked to draw while a render for the same page was still running. */
  overlaps: number[];
  /** Models the page's slot leaving the document, so its next draw gets a new canvas element. */
  detach(page: number): void;
  maxActive(): number;
  changes(): number;
} {
  const draws: FakeDraw[] = [];
  const releases: FakeRelease[] = [];
  const overlaps: number[] = [];
  const activePages = new Set<number>();
  const canvases = new Map<number, number>();
  let nextCanvas = 0;
  let changed = 0;
  let active = 0;
  let mostActive = 0;

  const ports: PageRenderPorts = {
    draw(page: number, scale: number): PageDraw {
      if (activePages.has(page)) overlaps.push(page);
      activePages.add(page);
      active += 1;
      mostActive = Math.max(mostActive, active);

      let canvas = canvases.get(page);
      if (canvas === undefined) {
        canvas = (nextCanvas += 1);
        canvases.set(page, canvas);
      }

      let settleDraw: ((error: Error | null) => void) | null = null;
      const promise = new Promise<void>((resolve, reject) => {
        settleDraw = (error) => (error === null ? resolve() : reject(error));
      });
      const draw: FakeDraw = {
        page,
        scale,
        canvas,
        cancelled: false,
        settled: false,
        released: false,
        settle: (reason?: string) => {
          if (draw.settled) return;
          draw.settled = true;
          active -= 1;
          activePages.delete(page);
          settleDraw?.(reason === undefined ? null : new ReaderPagesError(reason));
        },
      };
      draws.push(draw);
      return {
        promise,
        // Deliberately does not settle: the set must keep counting this job until the promise does.
        cancel: () => {
          draw.cancelled = true;
        },
        release: () => {
          if (draw.released) return;
          draw.released = true;
          releases.push({
            page,
            canvas,
            settled: draw.settled,
            whileDrawing: activePages.has(page),
          });
        },
      };
    },
    changed: () => {
      changed += 1;
    },
  };

  return {
    ports,
    draws,
    releases,
    overlaps,
    detach: (page) => {
      canvases.delete(page);
    },
    maxActive: () => mostActive,
    changes: () => changed,
  };
}

/** Settles every draw the set has started, including the ones it starts as a result, until it is idle. */
async function settleAll(world: { draws: FakeDraw[] }): Promise<void> {
  for (let round = 0; round < 20; round += 1) {
    const pending = world.draws.filter((draw) => !draw.settled);
    if (pending.length === 0) return;
    for (const draw of pending) draw.settle();
    await settle();
  }
}

describe("the continuous page stack", () => {
  it("places every page below the previous one, with one gap between them", () => {
    const layout = pageLayout(SIZES, { scale: 1, gap: GAP });

    expect(layout.boxes.map((box) => [box.page, box.top, box.height])).toEqual([
      [1, 0, 400],
      [2, 424, 400],
      [3, 848, 400],
      [4, 1272, 400],
    ]);
    // 4 pages of 400 plus 3 gaps: the scrollable height the stack has to claim.
    expect(layout.height).toBe(1672);
    expect(layout.width).toBe(300);
  });

  it("scales both dimensions and the gaps stay between pages at the new size", () => {
    const layout = pageLayout(SIZES, { scale: 2, gap: GAP });

    expect(layout.boxes.map((box) => [box.top, box.height, box.width])).toEqual([
      [0, 800, 600],
      [824, 800, 600],
      [1648, 800, 600],
      [2472, 800, 600],
    ]);
    expect(layout.height).toBe(3272);
  });

  it("centres a narrower page in the widest page's column", () => {
    const layout = pageLayout(
      [
        { width: 300, height: 400 },
        { width: 200, height: 300 },
      ],
      { scale: 1, gap: 10 },
    );

    expect(layout.width).toBe(300);
    expect(layout.boxes[1]).toMatchObject({ top: 410, left: 50, width: 200, height: 300 });
    expect(layout.height).toBe(710);
  });

  it("draws an empty document as an empty stack instead of failing", () => {
    const layout = pageLayout([], { scale: 1, gap: GAP });

    expect(layout.boxes).toEqual([]);
    expect(layout.height).toBe(0);
    expect(layout.width).toBe(0);
  });

  it("keeps its breathing room inside the stack, so scroll offsets and page coordinates share one origin", () => {
    const layout = pageLayout(SIZES, { scale: 1, gap: GAP, padding: 20 });

    // The padding is part of the laid-out stack, not the scroller's: page 1 starts 20 px into the scroll
    // content, and the stack claims the same 20 px at the bottom.
    expect(layout.boxes.map((box) => box.top)).toEqual([20, 444, 868, 1292]);
    expect(layout.height).toBe(1712);
    // Therefore a scroll offset is a page coordinate: at 900 the viewport reaches 1300 and page 4 starts
    // at 1292, so the page in the last eight pixels of the viewport is in the window.
    expect(
      pageWindow(layout, { scrollTop: 900, viewportHeight: 400, overscan: 0, maxPages: 2 }).visible,
    ).toEqual([3, 4]);
  });

  it.each([-1, Number.NaN, Number.POSITIVE_INFINITY])("refuses a %s padding", (padding) => {
    expect(() => pageLayout(SIZES, { scale: 1, gap: GAP, padding })).toThrow(ReaderPagesError);
  });

  it.each([0, -1, Number.NaN, Number.POSITIVE_INFINITY])(
    "refuses a scale it cannot lay out (%s)",
    (scale) => {
      expect(() => pageLayout(SIZES, { scale, gap: GAP })).toThrow(ReaderPagesError);
    },
  );

  it.each([
    ["a zero width", { width: 0, height: 400 }],
    ["a negative height", { width: 300, height: -1 }],
    ["a NaN size", { width: Number.NaN, height: 400 }],
    ["an infinite size", { width: 300, height: Number.POSITIVE_INFINITY }],
  ])("refuses a page with %s", (_name, size) => {
    expect(() => pageLayout([size], { scale: 1, gap: GAP })).toThrow(/page 1/);
  });

  it.each([-1, Number.NaN, Number.POSITIVE_INFINITY])("refuses a %s gap", (gap) => {
    expect(() => pageLayout(SIZES, { scale: 1, gap })).toThrow(ReaderPagesError);
  });
});

describe("the visible window of the stack", () => {
  const layout = pageLayout(SIZES, { scale: 1, gap: GAP });

  it("names every page the viewport touches, from one, ascending", () => {
    const window = pageWindow(layout, {
      scrollTop: 350,
      viewportHeight: 200,
      overscan: 0,
      maxPages: 4,
    });

    // Page 1 ends at 400 and page 2 runs 424–824, so both are inside 350–550.
    expect(window.visible).toEqual([1, 2]);
    expect(window.pages).toEqual([1, 2]);
    expect(window.start).toBe(0);
    expect(window.end).toBe(2);
  });

  it("never drops a page the viewport touches, whatever the page budget is", () => {
    // A viewport taller than the document: four pages are simultaneously visible and the budget is two.
    const window = pageWindow(layout, {
      scrollTop: 0,
      viewportHeight: 1672,
      overscan: 0,
      maxPages: 2,
    });

    // Visibility wins. The budget belongs to the render queue (how many draw at once), not to the
    // window: dropping a page the reader is looking at to save a canvas would show an empty gap.
    expect(window.visible).toEqual([1, 2, 3, 4]);
    expect(window.pages).toEqual([1, 2, 3, 4]);
  });

  it("grows the window with the overscan up to the page budget", () => {
    // The viewport band 350–400 is the last fifty pixels of page 1: page 2 starts at 424.
    const withoutOverscan = pageWindow(layout, {
      scrollTop: 350,
      viewportHeight: 50,
      overscan: 0,
      maxPages: 4,
    });
    const withOverscan = pageWindow(layout, {
      scrollTop: 350,
      viewportHeight: 50,
      // 500 px of overscan above and below reaches page 3's start at 848.
      overscan: 500,
      maxPages: 4,
    });

    expect(withoutOverscan.pages).toEqual([1]);
    expect(withoutOverscan.visible).toEqual([1]);
    expect(withOverscan.pages).toEqual([1, 2, 3]);
    expect(withOverscan.visible).toEqual([1]);
  });

  it("spends the spare budget on the page nearest the viewport, outside it", () => {
    const window = pageWindow(layout, {
      scrollTop: 400,
      viewportHeight: 400,
      overscan: 500,
      maxPages: 2,
    });

    // Only page 2 is in 400–800. Page 1's centre is 400 px from the viewport centre and page 3's is
    // 448, so the one spare slot goes to page 1.
    expect(window.visible).toEqual([2]);
    expect(window.pages).toEqual([1, 2]);
  });

  it("takes the earlier page when the overscan candidates are equally near", () => {
    const window = pageWindow(layout, {
      scrollTop: 424,
      viewportHeight: 400,
      overscan: 500,
      maxPages: 2,
    });

    // The viewport is exactly page 2, so pages 1 and 3 are both 424 px from its centre.
    expect(window.visible).toEqual([2]);
    expect(window.pages).toEqual([1, 2]);
  });

  it("adds overscan pages on both sides while the budget allows", () => {
    const window = pageWindow(layout, {
      scrollTop: 424,
      viewportHeight: 400,
      overscan: 500,
      maxPages: 3,
    });

    expect(window.visible).toEqual([2]);
    expect(window.pages).toEqual([1, 2, 3]);
  });

  it("always names one page, even when the viewport sits in a gap", () => {
    const gapped = pageLayout(
      [
        { width: 300, height: 100 },
        { width: 300, height: 100 },
      ],
      { scale: 1, gap: 100 },
    );

    // 150 is page one's gap: nothing intersects, and page 2 (centre 250) is nearer than page 1 (50).
    const window = pageWindow(gapped, {
      scrollTop: 150,
      viewportHeight: 10,
      overscan: 0,
      maxPages: 2,
    });

    expect(window.visible).toEqual([2]);
    expect(window.pages).toEqual([2]);
  });

  it("names no page for an empty stack", () => {
    const empty = pageLayout([], { scale: 1, gap: GAP });

    expect(pageWindow(empty, { scrollTop: 0, viewportHeight: 400, overscan: 0, maxPages: 4 }))
      .toEqual({ start: 0, end: 0, pages: [], visible: [] });
  });

  it("reads a negative scroll offset as the top of the document", () => {
    const window = pageWindow(layout, {
      scrollTop: -50,
      viewportHeight: 100,
      overscan: 0,
      maxPages: 4,
    });

    expect(window.visible).toEqual([1]);
    expect(window.pages).toEqual([1]);
  });

  it.each([0, -1, 1.5, Number.NaN])("refuses a %s page bound", (maxPages) => {
    expect(() =>
      pageWindow(layout, { scrollTop: 0, viewportHeight: 400, overscan: 0, maxPages }),
    ).toThrow(ReaderPagesError);
  });

  it.each([
    ["a NaN scroll offset", { scrollTop: Number.NaN }],
    ["a negative viewport", { viewportHeight: -1 }],
    ["a NaN overscan", { overscan: Number.NaN }],
  ])("refuses %s", (_name, override) => {
    expect(() =>
      pageWindow(layout, {
        scrollTop: 0,
        viewportHeight: 400,
        overscan: 0,
        maxPages: 4,
        ...override,
      }),
    ).toThrow(ReaderPagesError);
  });
});

describe("pages whose size has not been measured", () => {
  it("lays an unmeasured page out at the last measured size and marks the box as estimated", () => {
    const layout = pageLayout([SIZES[0], SIZES[1], null, null], { scale: 1, gap: GAP });

    expect(layout.boxes.map((box) => [box.page, box.width, box.height, box.estimated])).toEqual([
      [1, 300, 400, false],
      [2, 300, 400, false],
      [3, 300, 400, true],
      [4, 300, 400, true],
    ]);
  });

  it("uses a measured size wherever one exists, however different it is", () => {
    const layout = pageLayout([SIZES[0], { width: 600, height: 200 }, null], { scale: 1, gap: GAP });

    // Page 2's real 3:1 aspect is kept, and the unmeasured page after it borrows that, not the first page.
    expect(layout.boxes.map((box) => [box.width, box.height, box.estimated])).toEqual([
      [300, 400, false],
      [600, 200, false],
      [600, 200, true],
    ]);
    expect(layout.boxes[1].width / layout.boxes[1].height).toBe(3);
  });

  it("falls back to the default page size when nothing at all has been measured", () => {
    const layout = pageLayout([null, null], { scale: 1, gap: GAP });

    expect(layout.boxes[0]).toMatchObject({
      top: 0,
      width: DEFAULT_PAGE_SIZE.width,
      height: DEFAULT_PAGE_SIZE.height,
      estimated: true,
    });
  });

  it("replaces one page's size without touching the pages around it", () => {
    const sizes: readonly (PageSize | null)[] = [SIZES[0], null, null];

    const next = withPageSize(sizes, 2, { width: 600, height: 200 });

    expect(next).not.toBe(sizes);
    expect(next).toEqual([SIZES[0], { width: 600, height: 200 }, null]);
    // The entries that did not change are the same objects: only one page was acquired.
    expect(next[0]).toBe(sizes[0]);
    expect(() => withPageSize(sizes, 0, SIZES[0])).toThrow(ReaderPagesError);
    expect(() => withPageSize(sizes, 4, SIZES[0])).toThrow(ReaderPagesError);
  });

  it("corrects a mixed-aspect page past the measurement cap, and keeps the reader where they were", () => {
    // Fourteen pages instead of 514 so the arithmetic is readable; page 3 stands in for page 513: the
    // layout has no size for it, and its real one is landscape.
    const sizes: readonly (PageSize | null)[] = [SIZES[0], SIZES[1], null, SIZES[3]];
    const before = pageLayout(sizes, { scale: 1, gap: GAP });

    // A 600x200 page must never be drawn into page 3's estimated 300x400 box: that would stretch it.
    expect(before.boxes[2]).toMatchObject({ width: 300, height: 400, estimated: true });
    const page4Mid = before.boxes[3].top + before.boxes[3].height / 2;
    const anchor = anchorAt(before, page4Mid);
    expect(anchor).toEqual({ page: 4, ratio: 0.5 });
    if (anchor === null) throw new Error("page 4 has no anchor");

    const corrected = pageLayout(withPageSize(sizes, 3, { width: 600, height: 200 }), {
      scale: 1,
      gap: GAP,
    });

    expect(corrected.boxes[2]).toMatchObject({
      width: 600,
      height: 200,
      estimated: false,
    });
    expect(corrected.boxes[2].width / corrected.boxes[2].height).toBe(3);

    // Page 4 moved up by the difference; the anchor puts the reader back in the middle of it.
    const restored = offsetForAnchor(corrected, anchor);
    expect(restored).toBe(corrected.boxes[3].top + corrected.boxes[3].height / 2);
    expect(restored).not.toBe(page4Mid);
  });
});

describe("zooming", () => {
  it("steps in and out by one factor, without floating-point dust", () => {
    expect(zoomScale(1, "in")).toBe(1.25);
    expect(zoomScale(1.25, "out")).toBe(1);
    expect(zoomScale(2, "in")).toBe(2.5);
  });

  it("stops at the bounds instead of running past them", () => {
    expect(zoomScale(MAX_SCALE, "in")).toBe(MAX_SCALE);
    expect(zoomScale(MIN_SCALE, "out")).toBe(MIN_SCALE);
    // 3.9 × 1.25 is 4.875: the clamp, not the multiplication, decides.
    expect(zoomScale(3.9, "in")).toBe(MAX_SCALE);
  });

  it("clamps a scale inside the range and refuses one that is not a scale", () => {
    expect(clampScale(0.1)).toBe(MIN_SCALE);
    expect(clampScale(10)).toBe(MAX_SCALE);
    expect(clampScale(1.5)).toBe(1.5);
    expect(() => clampScale(0)).toThrow(ReaderPagesError);
    expect(() => clampScale(Number.NaN)).toThrow(ReaderPagesError);
  });
});

describe("one page's canvas buffer", () => {
  it("draws at the screen's device pixel ratio, bounded by the maximum", () => {
    expect(canvasBuffer(300, 400, 1)).toEqual({ width: 300, height: 400, ratio: 1 });
    expect(canvasBuffer(300, 400, 1.5)).toEqual({ width: 450, height: 600, ratio: 1.5 });
    // A 3x screen is capped, so a zoomed page cannot ask for nine times the memory.
    expect(canvasBuffer(300, 400, 3)).toEqual({
      width: 300 * MAX_DEVICE_PIXEL_RATIO,
      height: 400 * MAX_DEVICE_PIXEL_RATIO,
      ratio: MAX_DEVICE_PIXEL_RATIO,
    });
  });

  it("lowers the ratio rather than exceed the pixel budget", () => {
    const buffer = canvasBuffer(1600, 1200, 2);

    expect(buffer.ratio).toBeLessThan(2);
    expect(buffer.width * buffer.height).toBeLessThanOrEqual(MAX_CANVAS_PIXELS);
  });

  it("rounds a fractional CSS pixel up, so the last column is not clipped", () => {
    expect(canvasBuffer(100.2, 50.4, 1)).toEqual({ width: 101, height: 51, ratio: 1 });
  });

  it("refuses a page or a ratio it cannot draw", () => {
    expect(() => canvasBuffer(0, 400, 1)).toThrow(ReaderPagesError);
    expect(() => canvasBuffer(300, 400, 0)).toThrow(ReaderPagesError);
    expect(() => canvasBuffer(300, Number.NaN, 1)).toThrow(ReaderPagesError);
  });
});

describe("keeping the reading position across a zoom", () => {
  const layout = pageLayout(SIZES, { scale: 1, gap: GAP });

  /** The anchor, failing the test rather than the type check when there is no page to name. */
  function anchorOrFail(offset: number): ScrollAnchor {
    const anchor = anchorAt(layout, offset);
    if (anchor === null) throw new Error(`no page to anchor to at ${offset}`);
    return anchor;
  }

  it("names the page and the fraction of it under the scroll offset", () => {
    // Page 2 runs 424–824, so 624 is half way down it.
    expect(anchorAt(layout, 624)).toEqual({ page: 2, ratio: 0.5 });
    expect(anchorAt(layout, 424)).toEqual({ page: 2, ratio: 0 });
  });

  it("reads an offset before the first page as its top", () => {
    expect(anchorAt(layout, -20)).toEqual({ page: 1, ratio: 0 });
  });

  it("returns to the same page and fraction in the new layout", () => {
    const anchor = anchorOrFail(624);
    const zoomed = pageLayout(SIZES, { scale: 2, gap: GAP });

    // The anchor is page 2's middle; at 2x that page runs 824–1624.
    expect(offsetForAnchor(zoomed, anchor)).toBe(1224);
  });

  it("round-trips an offset that is already in the layout", () => {
    for (const offset of [0, 100, 500, 1300, 1672]) {
      expect(offsetForAnchor(layout, anchorOrFail(offset))).toBeCloseTo(offset, 6);
    }
  });

  it("answers nothing when there is no page to anchor to", () => {
    const empty = pageLayout([], { scale: 1, gap: GAP });

    expect(anchorAt(empty, 0)).toBeNull();
    expect(offsetForAnchor(empty, { page: 1, ratio: 0.5 })).toBeNull();
    expect(offsetForAnchor(layout, { page: 9, ratio: 0 })).toBeNull();
  });
});

describe("reading the pages' sizes", () => {
  const PAGES: readonly PageSize[] = [
    { width: 300, height: 400 },
    { width: 200, height: 500 },
    { width: 999, height: 999 },
    { width: 999, height: 999 },
  ];

  /** A measurement port that answers from a table and records which pages it was asked for. */
  function fakeMeasure(sizes: readonly PageSize[]): {
    measure: PageSizeMeasure;
    asked: number[];
  } {
    const asked: number[] = [];
    return {
      asked,
      measure: async (page) => {
        asked.push(page);
        return sizes[page - 1];
      },
    };
  }

  it("measures the leading pages and leaves the rest unknown instead of inventing a size", async () => {
    const { measure, asked } = fakeMeasure(PAGES);

    const progress = await readPageSizes(measure, 4, { maxPages: 2 });

    expect(asked).toEqual([1, 2]);
    expect(progress.measured).toBe(2);
    // `null`, not a copy of page 2: a copy would silently give a differently shaped page the wrong aspect
    // and stretch it until something corrected it.
    expect(progress.sizes).toEqual([PAGES[0], PAGES[1], null, null]);
  });

  it("measures every page when the document fits the bound", async () => {
    const { measure, asked } = fakeMeasure(PAGES);

    const progress = await readPageSizes(measure, 4);

    expect(asked).toEqual([1, 2, 3, 4]);
    expect(progress.measured).toBe(4);
    expect(progress.sizes).toEqual(PAGES);
  });

  it("stops asking for pages as soon as it is aborted", async () => {
    const controller = new AbortController();
    const asked: number[] = [];
    const measure: PageSizeMeasure = async (page) => {
      asked.push(page);
      if (page === 2) controller.abort();
      return PAGES[0];
    };

    await expect(readPageSizes(measure, 5, { signal: controller.signal })).rejects.toBeInstanceOf(
      MeasurementStopped,
    );
    expect(asked).toEqual([1, 2]);
  });

  it("discards a page that arrives after the abort instead of asking for the next one", async () => {
    const controller = new AbortController();
    const started: number[] = [];
    const gate: Array<() => void> = [];
    const measure: PageSizeMeasure = (page) =>
      new Promise<PageSize>((resolve) => {
        started.push(page);
        gate.push(() => resolve(PAGES[page - 1]));
      });

    const progress = readPageSizes(measure, 5, { signal: controller.signal });
    // The first page arrives, so the second is asked for and left in flight.
    gate[0]();
    await settle();
    expect(started).toEqual([1, 2]);

    controller.abort();
    gate[1]();
    await expect(progress).rejects.toBeInstanceOf(MeasurementStopped);
    expect(started).toEqual([1, 2]);
  });

  it("refuses a page count or a measurement bound that cannot be used", async () => {
    const { measure } = fakeMeasure(PAGES);

    await expect(readPageSizes(measure, 0)).rejects.toBeInstanceOf(ReaderPagesError);
    await expect(readPageSizes(measure, Number.NaN)).rejects.toBeInstanceOf(ReaderPagesError);
    await expect(readPageSizes(measure, 4, { maxPages: 0 })).rejects.toBeInstanceOf(
      ReaderPagesError,
    );
  });

  it("reports a page whose reported size cannot be laid out, naming it", async () => {
    const measure: PageSizeMeasure = async () => ({ width: 0, height: 400 });

    await expect(readPageSizes(measure, 1)).rejects.toThrow(/page 1/);
  });

  it("reports a page that could not be measured instead of inventing a size", async () => {
    const measure: PageSizeMeasure = async () => {
      throw new Error("the page dictionary is unreadable");
    };

    await expect(readPageSizes(measure, 1)).rejects.toThrow(/unreadable/);
  });
});

describe("the render set", () => {
  it("draws the window in order, up to the concurrency bound", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1, 2, 3], 1);

    expect(world.draws.map((draw) => draw.page)).toEqual([1, 2]);
    expect(set.snapshot().drawing).toEqual([1, 2]);

    world.draws[0].settle();
    await settle();

    // Finishing page 1 is what makes room for page 3.
    expect(world.draws.map((draw) => draw.page)).toEqual([1, 2, 3]);
    expect(set.snapshot().drawn).toEqual([1]);
    expect(world.maxActive()).toBe(2);
  });

  it("never runs more jobs than its bound, and keeps the pages in window order", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports, { maxConcurrent: 1 });

    set.setView([3, 4], 1);
    expect(world.draws.map((draw) => draw.page)).toEqual([3]);

    world.draws[0].settle();
    await settle();

    expect(world.draws.map((draw) => draw.page)).toEqual([3, 4]);
    expect(world.maxActive()).toBe(1);
  });

  it("keeps a cancelled job counted until its own promise settles", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports, { maxConcurrent: 1 });

    set.setView([1], 1);
    set.setView([2], 1);

    // Page 1 was cancelled, but its render has not stopped: the slot is not free and page 2 waits.
    expect(world.draws[0].cancelled).toBe(true);
    expect(world.draws.map((draw) => draw.page)).toEqual([1]);
    expect(set.snapshot().drawing).toEqual([]);
    expect(set.snapshot().drawn).toEqual([]);

    world.draws[0].settle("cancelled");
    await settle();

    expect(world.draws.map((draw) => draw.page)).toEqual([1, 2]);
    expect(set.snapshot().failed).toEqual([]);
  });

  it("never reuses a page's canvas while that page's previous render is still settling", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1], 1);
    set.setView([1], 2);

    // Same page, new scale: the old render is cancelled but still running, so no second draw may start
    // and nothing may be released yet.
    expect(world.draws).toHaveLength(1);
    expect(world.draws[0].cancelled).toBe(true);
    expect(world.releases).toEqual([]);

    world.draws[0].settle("cancelled");
    await settle();

    expect(world.overlaps).toEqual([]);
    expect(world.draws.map((draw) => [draw.page, draw.scale])).toEqual([
      [1, 1],
      [1, 2],
    ]);
    // The old canvas went back after the old render stopped, and before the new render started.
    expect(world.releases).toEqual([{ page: 1, canvas: 1, settled: true, whileDrawing: false }]);

    world.draws[1].settle();
    await settle();
    expect(set.snapshot().drawn).toEqual([1]);
    expect(set.snapshot().scale).toBe(2);
  });

  it("waits for a settling render before drawing that page again, even with a free slot", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports, { maxConcurrent: 2 });

    set.setView([2], 1);
    set.setView([1], 1); // page 2 is cancelled, page 1 gets the free slot
    set.setView([1, 2], 1); // page 2 is wanted again, but its old render is still settling

    expect(world.draws.map((draw) => draw.page)).toEqual([2, 1]);

    world.draws[0].settle("cancelled");
    await settle();

    expect(world.draws.map((draw) => [draw.page, draw.scale])).toEqual([
      [2, 1],
      [1, 1],
      [2, 1],
    ]);
    expect(world.overlaps).toEqual([]);
    expect(world.maxActive()).toBe(2);
  });

  it("survives repeated scroll and zoom churn without overlapping or exceeding its bound", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports, { maxConcurrent: 2 });

    set.setView([1, 2], 1);
    set.setView([2, 3], 1);
    set.setView([2, 3], 2);
    set.setView([3, 4], 2);

    // Settle the cancellations that are still counted; each one frees a slot for the last view.
    for (const draw of world.draws.filter((entry) => entry.cancelled && !entry.settled)) {
      draw.settle("cancelled");
    }
    await settle();
    await settleAll(world);

    expect(world.overlaps).toEqual([]);
    expect(world.maxActive()).toBe(2);
    expect(set.snapshot().window).toEqual([3, 4]);
    expect(set.snapshot().drawn).toEqual([3, 4]);
    expect(set.snapshot().scale).toBe(2);
  });

  it("releases a cancelled page's resources only after its render has really stopped", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1], 1);
    set.setView([], 1);

    expect(world.releases).toEqual([]);

    world.draws[0].settle("cancelled");
    await settle();

    expect(world.releases).toEqual([{ page: 1, canvas: 1, settled: true, whileDrawing: false }]);
    expect(set.snapshot().failed).toEqual([]);
    expect(set.snapshot().drawn).toEqual([]);
  });

  it("releases a drawn canvas once when its page leaves the window", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1, 2], 1);
    world.draws[0].settle();
    await settle();
    world.draws[1].settle();
    await settle();
    expect(set.snapshot().drawn).toEqual([1, 2]);

    set.setView([2], 1);
    set.setView([2], 1);

    expect(world.releases).toEqual([{ page: 1, canvas: 1, settled: true, whileDrawing: false }]);
    expect(set.snapshot().drawn).toEqual([2]);
  });

  it("frees the old canvas and draws again when the zoom changes", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1], 1);
    world.draws[0].settle();
    await settle();
    expect(set.snapshot().drawn).toEqual([1]);

    set.setView([1], 2);

    // A drawn page is idle, so nothing has to be awaited: the old-scale bitmap is released and the new
    // render starts at once.
    expect(world.releases).toEqual([{ page: 1, canvas: 1, settled: true, whileDrawing: false }]);
    expect(set.snapshot().drawn).toEqual([]);
    expect(world.draws.map((draw) => [draw.page, draw.scale])).toEqual([
      [1, 1],
      [1, 2],
    ]);

    world.draws[1].settle();
    await settle();
    expect(set.snapshot().drawn).toEqual([1]);
  });

  it("reports a failed page honestly, releases it, and retries only when asked", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1], 1);
    world.draws[0].settle("the page stream is broken");
    await settle();

    expect(set.snapshot().failed).toEqual([{ page: 1, reason: "the page stream is broken" }]);
    expect(set.snapshot().drawn).toEqual([]);
    // A failed render leaves nothing worth keeping on the canvas.
    expect(world.releases).toEqual([{ page: 1, canvas: 1, settled: true, whileDrawing: false }]);

    // A window update alone must not loop on the same broken page.
    set.setView([1], 1);
    await settle();
    expect(world.draws).toHaveLength(1);

    set.retry(1);
    expect(world.draws).toHaveLength(2);
    world.draws[1].settle();
    await settle();

    expect(set.snapshot().failed).toEqual([]);
    expect(set.snapshot().drawn).toEqual([1]);
  });

  it("forgets a failure when the zoom changes, so the page is tried at the new size", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1], 1);
    world.draws[0].settle("the page stream is broken");
    await settle();
    set.setView([1], 1.25);

    expect(set.snapshot().failed).toEqual([]);
    expect(world.draws.map((draw) => draw.scale)).toEqual([1, 1.25]);
  });

  it("reports a draw that throws synchronously as a failure, and releases what it may have taken", async () => {
    const world = fakePorts();
    const ports: PageRenderPorts = {
      ...world.ports,
      draw: () => {
        throw new ReaderPagesError("the canvas is gone");
      },
    };
    const set = new PageRenderSet(ports);

    expect(() => set.setView([1], 1)).not.toThrow();
    await settle();

    expect(set.snapshot().failed).toEqual([{ page: 1, reason: "the canvas is gone" }]);
    // There is no handle to release: a draw that throws before returning one never gave the set anything
    // to hand back, so the port is responsible for whatever it touched before failing.
    expect(world.releases).toEqual([]);
  });

  it("does not draw a page twice for the same window and scale", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1, 2], 1);
    world.draws[0].settle();
    await settle();
    set.setView([1, 2], 1);
    set.setView([1, 2], 1);
    await settle();

    expect(world.draws.map((draw) => draw.page)).toEqual([1, 2]);
  });

  it("queues every page of a tall window instead of dropping the ones over the budget", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1, 2, 3, 4, 5, 6], 1);
    expect(world.draws.map((draw) => draw.page)).toEqual([1, 2]);

    await settleAll(world);

    expect(world.draws.map((draw) => draw.page)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(set.snapshot().drawn).toEqual([1, 2, 3, 4, 5, 6]);
    expect(world.maxActive()).toBe(2);
    expect(world.overlaps).toEqual([]);
  });

  it("stops drawing once disposed, but still releases what cancellation left behind", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1, 2], 1);
    world.draws[0].settle();
    await settle();
    expect(set.snapshot().drawn).toEqual([1]);

    set.dispose();

    // The drawn page is released now; the running render is cancelled, and its resources are released
    // only once it has really stopped.
    expect(world.releases).toEqual([{ page: 1, canvas: 1, settled: true, whileDrawing: false }]);
    expect(world.draws[1].cancelled).toBe(true);
    expect(set.snapshot()).toEqual({ scale: 1, window: [], drawn: [], drawing: [], failed: [] });

    const changesAfterDispose = world.changes();
    set.setView([1], 1);
    set.retry(1);
    expect(world.draws).toHaveLength(2);
    expect(world.changes()).toBe(changesAfterDispose);

    world.draws[1].settle("cancelled");
    await settle();

    expect(world.releases.map((release) => [release.page, release.canvas])).toEqual([
      [1, 1],
      [2, 2],
    ]);
    expect(world.changes()).toBe(changesAfterDispose);
    expect(world.draws).toHaveLength(2);
  });

  it("releases the canvas its own render used, not the page's newer canvas", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([1], 1); // draw 0 into canvas 1
    set.setView([1], 2); // cancelled while still running
    expect(world.releases).toEqual([]);

    // The page left the window and came back with a new canvas element while the old render settles.
    set.setView([], 2);
    world.detach(1);
    set.setView([1], 2);
    expect(world.draws).toHaveLength(1); // page 1 is still occupied by the settling render

    world.draws[0].settle("cancelled");
    await settle();

    // The release names canvas 1, the one the cancelled draw was using. Canvas 2 is untouched.
    expect(world.releases).toEqual([
      { page: 1, canvas: 1, settled: true, whileDrawing: false },
    ]);
    expect(world.draws.map((draw) => [draw.page, draw.scale, draw.canvas])).toEqual([
      [1, 1, 1],
      [1, 2, 2],
    ]);
    expect(world.overlaps).toEqual([]);
  });

  it("knows which pages a render is using, so measurement cannot clean a proxy under it", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    expect(set.isInUse(1)).toBe(false);

    set.setView([1, 2], 1);
    expect(set.isInUse(1)).toBe(true); // drawing
    world.draws[0].settle();
    await settle();
    expect(set.isInUse(1)).toBe(true); // drawn

    set.setView([2], 1);
    expect(set.isInUse(1)).toBe(false); // evicted and released

    set.setView([2], 2);
    expect(set.isInUse(2)).toBe(true); // cancelled, still settling
    world.draws[1].settle("cancelled");
    await settle();
    await settleAll(world);
    expect(set.isInUse(2)).toBe(true); // drawn at the new scale
    expect(set.isInUse(3)).toBe(false);
  });

  it("refuses a page number that cannot be a page", () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    expect(() => set.setView([0], 1)).toThrow(ReaderPagesError);
    expect(() => set.setView([1.5], 1)).toThrow(ReaderPagesError);
    expect(world.draws).toEqual([]);
  });

  it("draws a page once when the window names it twice", () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([2, 2], 1);

    expect(world.draws.map((draw) => draw.page)).toEqual([2]);
    expect(set.snapshot().window).toEqual([2]);
  });

  it("refuses a scale it cannot render at", () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    expect(() => set.setView([1], 0)).toThrow(ReaderPagesError);
    expect(() => set.setView([1], Number.NaN)).toThrow(ReaderPagesError);
    expect(world.draws).toEqual([]);
  });

  it.each([0, -1, 1.5, Number.NaN])("refuses a %s concurrency bound", (maxConcurrent) => {
    expect(() => new PageRenderSet(fakePorts().ports, { maxConcurrent })).toThrow(
      ReaderPagesError,
    );
  });

  it("answers each snapshot as a fresh, sorted view of its own state", async () => {
    const world = fakePorts();
    const set = new PageRenderSet(world.ports);

    set.setView([3, 1], 1);
    world.draws[0].settle();
    world.draws[1].settle();
    await settle();

    const snapshot = set.snapshot();
    expect(snapshot).not.toBe(set.snapshot());
    expect(snapshot.drawn).toEqual([1, 3]);
    // Mutating what a caller was handed must not reach back into the set.
    (snapshot.drawn as number[]).push(99);
    expect(set.snapshot().drawn).toEqual([1, 3]);
  });
});

describe("one document's session", () => {
  interface FakeDocument {
    readonly document: ReaderDocument;
    /** Every page the document was asked for, in order. */
    readonly asked: number[];
    readonly renderCalls: Array<{
      readonly page: number;
      readonly width: number;
      readonly height: number;
      readonly canvas: HTMLCanvasElement | null | undefined;
    }>;
    readonly cleanups: number[];
  }

  interface Deferred<T> {
    readonly promise: Promise<T>;
    resolve(value: T): void;
  }

  function deferred<T>(): Deferred<T> {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((settle) => {
      resolve = settle;
    });
    return { promise, resolve };
  }

  /**
   * One document, behaving like pdf.js: `getPage` answers the SAME cached proxy promise for every call,
   * and different documents have different proxies for the same page number. A gate can hold the
   * acquisition or the render so an old document can be replaced while its work is still in flight.
   */
  function fakeDocumentInstance(
    sizes: Readonly<Record<number, PageSize>>,
    gates: { readonly getPage?: Promise<void>; readonly render?: Promise<void> } = {},
  ): FakeDocument {
    const cache = new Map<number, Promise<ReaderPage>>();
    const asked: number[] = [];
    const renderCalls: FakeDocument["renderCalls"] = [];
    const cleanups: number[] = [];

    const document: ReaderDocument = {
      numPages: 4,
      getPage(page: number): Promise<ReaderPage> {
        asked.push(page);
        let proxy = cache.get(page);
        if (proxy === undefined) {
          proxy = (async () => {
            await gates.getPage;
            const size = sizes[page];
            const pageObject: ReaderPage = {
              getViewport: ({ scale }) => ({ width: size.width * scale, height: size.height * scale }),
              render: (request) => {
                renderCalls.push({
                  page,
                  width: request.viewport.width,
                  height: request.viewport.height,
                  canvas: request.canvas ?? null,
                });
                return { promise: gates.render ?? Promise.resolve(), cancel: () => {} };
              },
              cleanup: () => {
                cleanups.push(page);
              },
            };
            return pageObject;
          })();
          cache.set(page, proxy);
        }
        return proxy;
      },
    };
    return { document, asked, renderCalls, cleanups };
  }

  /** The only members the session touches; the rest of a canvas is irrelevant to ownership. */
  function fakeCanvas(): HTMLCanvasElement {
    return { width: 0, height: 0, style: {} } as unknown as HTMLCanvasElement;
  }

  function sessionFor(
    world: FakeDocument,
    generation: number,
    options: Partial<PageSessionOptions> = {},
  ): PageSession {
    return new PageSession({
      document: world.document,
      generation,
      pixelRatio: 1,
      canvas: () => undefined,
      changed: () => {},
      ...options,
    });
  }

  it("returns one proxy for repeated acquisitions, the way pdf.js caches pages", async () => {
    const world = fakeDocumentInstance({ 2: { width: 300, height: 400 } });
    const session = sessionFor(world, 1);

    const first = session.pageProxy(2);
    const second = session.pageProxy(2);

    expect(second).toBe(first);
    expect(world.asked).toEqual([2]);
    await first;
  });

  it("keeps two documents' same-numbered pages apart", async () => {
    const oldWorld = fakeDocumentInstance({ 3: { width: 300, height: 400 } });
    const newWorld = fakeDocumentInstance({ 3: { width: 600, height: 200 } });
    const oldSession = sessionFor(oldWorld, 1);
    const newSession = sessionFor(newWorld, 2);

    const oldPage = await oldSession.pageProxy(3);
    const newPage = await newSession.pageProxy(3);

    // Same page number, same proxy promise inside one document, different pages across documents.
    expect(newPage).not.toBe(oldPage);
    expect(oldPage.getViewport({ scale: 1 })).toEqual({ width: 300, height: 400 });
    expect(newPage.getViewport({ scale: 1 })).toEqual({ width: 600, height: 200 });
    expect(oldWorld.asked).toEqual([3]);
    expect(newWorld.asked).toEqual([3]);
  });

  it("refuses to acquire pages once retired, so a late pass cannot start new work", () => {
    const world = fakeDocumentInstance({ 1: { width: 300, height: 400 } });
    const session = sessionFor(world, 1);

    session.retire();

    expect(session.retired).toBe(true);
    expect(() => session.pageProxy(1)).toThrow(ReaderPagesError);
    expect(world.asked).toEqual([]);
  });

  it("does not ask the document for a page once the session is retired", async () => {
    const world = fakeDocumentInstance({ 1: { width: 300, height: 400 } });
    const session = sessionFor(world, 1);

    session.retire();

    // A measurement pass that is still running must not acquire anything after retirement: no claim and,
    // above all, no `getPage` on a document that may already be destroyed.
    await expect(session.measurePageSize(1)).rejects.toBeInstanceOf(ReaderPagesError);
    expect(world.asked).toEqual([]);
  });

  it("cannot let a late old measurement clean the new document's proxy", async () => {
    const getPageGate = deferred<void>();
    const oldWorld = fakeDocumentInstance(
      { 3: { width: 300, height: 400 } },
      { getPage: getPageGate.promise },
    );
    const newWorld = fakeDocumentInstance({ 3: { width: 600, height: 200 } });
    const oldSession = sessionFor(oldWorld, 1);
    const newSession = sessionFor(newWorld, 2);

    const oldMeasurement = oldSession.measurePageSize(3);
    expect(oldWorld.asked).toEqual([3]);

    // The document is replaced while the old acquisition is still in flight.
    oldSession.retire();
    oldSession.releaseMeasuredPages();
    const newPage = await newSession.pageProxy(3);
    expect(newPage.getViewport({ scale: 1 })).toEqual({ width: 600, height: 200 });

    // The old page finally arrives: the old pass rejects it, and its cleanup touches only its own page.
    getPageGate.resolve();
    await expect(oldMeasurement).rejects.toBeInstanceOf(ReaderPagesError);
    oldSession.releaseMeasuredPages();
    await settle();

    expect(oldWorld.cleanups).toEqual([3]);
    expect(newWorld.cleanups).toEqual([]);
    await expect(newSession.pageProxy(3)).resolves.toBe(newPage);
    expect(newWorld.asked).toEqual([3]);
  });

  it("cannot render the old document's page after the document was replaced", async () => {
    const getPageGate = deferred<void>();
    const oldWorld = fakeDocumentInstance(
      { 1: { width: 300, height: 400 } },
      { getPage: getPageGate.promise },
    );
    const newWorld = fakeDocumentInstance({ 1: { width: 600, height: 200 } });
    const oldCanvas = fakeCanvas();
    const newCanvas = fakeCanvas();
    const oldSession = sessionFor(oldWorld, 1, {
      canvas: (page) => (page === 1 ? oldCanvas : undefined),
    });
    const newSession = sessionFor(newWorld, 2, {
      canvas: (page) => (page === 1 ? newCanvas : undefined),
    });

    oldSession.renders.setView([1], 1);
    expect(oldWorld.asked).toEqual([1]);

    oldSession.retire();
    newSession.renders.setView([1], 1);
    await settle();
    expect(newWorld.renderCalls).toHaveLength(1);
    expect(newWorld.renderCalls[0].canvas).toBe(newCanvas);

    getPageGate.resolve();
    await settle();

    // The old draw stopped before pdf.js, and the new document's canvas was never the old draw's target.
    expect(oldWorld.renderCalls).toEqual([]);
    expect(newCanvas.width).toBe(600);
  });

  it("releases the old document's canvas, never the new document's canvas for the same page", async () => {
    const renderGate = deferred<void>();
    const oldWorld = fakeDocumentInstance(
      { 1: { width: 300, height: 400 } },
      { render: renderGate.promise },
    );
    const newWorld = fakeDocumentInstance({ 1: { width: 600, height: 200 } });
    const oldCanvas = fakeCanvas();
    const newCanvas = fakeCanvas();
    const oldSession = sessionFor(oldWorld, 1, {
      canvas: (page) => (page === 1 ? oldCanvas : undefined),
    });
    const newSession = sessionFor(newWorld, 2, {
      canvas: (page) => (page === 1 ? newCanvas : undefined),
    });

    oldSession.renders.setView([1], 1);
    await settle();
    expect(oldCanvas.width).toBe(300);

    oldSession.retire();
    newSession.renders.setView([1], 1);
    await settle();
    expect(newCanvas.width).toBe(600);

    // The old render finally stops and its handle is released: only the canvas it drew into is zeroed.
    renderGate.resolve();
    await settle();

    expect(oldCanvas.width).toBe(0);
    expect(newCanvas.width).toBe(600);
    expect(oldWorld.cleanups).toEqual([1]);
    expect(newWorld.cleanups).toEqual([]);
  });

  it("releases measured pages except the ones a live draw is using", async () => {
    const renderGate = deferred<void>();
    const world = fakeDocumentInstance(
      { 1: { width: 300, height: 400 } },
      { render: renderGate.promise },
    );
    const canvas = fakeCanvas();
    const session = sessionFor(world, 1, { canvas: (page) => (page === 1 ? canvas : undefined) });

    await session.measurePageSize(1);
    session.renders.setView([1], 1);
    await settle();
    expect(world.cleanups).toEqual([]);

    // The draw holds the page, so the measurement cleanup must not pull it out from under the render.
    session.releaseMeasuredPages();
    expect(world.cleanups).toEqual([]);

    session.retire();
    renderGate.resolve();
    await settle();

    expect(world.cleanups).toEqual([1]);
  });

  it("records a page with no canvas as a failure without taking the page", async () => {
    const world = fakeDocumentInstance({ 1: { width: 300, height: 400 } });
    const session = sessionFor(world, 1);

    session.renders.setView([1], 1);
    await settle();

    expect(session.renders.snapshot().failed).toEqual([
      { page: 1, reason: "page 1 has no canvas to draw into" },
    ]);
    expect(world.asked).toEqual([]);
  });

  it("reports the page's own size when it is acquired, not the zoomed one", async () => {
    const world = fakeDocumentInstance({ 1: { width: 300, height: 400 } });
    const seen: Array<[number, PageSize]> = [];
    const session = sessionFor(world, 1, {
      canvas: () => fakeCanvas(),
      measured: (page, size) => seen.push([page, size]),
    });

    session.renders.setView([1], 2);
    await settle();

    expect(seen).toEqual([[1, { width: 300, height: 400 }]]);
    expect(world.renderCalls[0]).toMatchObject({ width: 600, height: 800 });
  });
});

describe("the deferred scroll restore guard", () => {
  it("keeps only the newest scheduled restore", () => {
    const guard = new ScrollRestoreGuard();

    const first = guard.begin();
    const second = guard.begin();

    expect(guard.isCurrent(first)).toBe(false);
    expect(guard.isCurrent(second)).toBe(true);
  });

  it("drops a pending restore once the reader scrolls or the reader/document goes away", () => {
    const guard = new ScrollRestoreGuard();
    const token = guard.begin();

    guard.invalidate();

    expect(guard.isCurrent(token)).toBe(false);
    // A restore scheduled after the invalidation is current again.
    expect(guard.isCurrent(guard.begin())).toBe(true);
  });
});

describe("page ownership across readers of one document", () => {
  interface CleanupWorld {
    readonly document: ReaderDocument;
    readonly cleanupCalls: number[];
    readonly renderCalls: number[];
    page(page: number): ReaderPage | undefined;
  }

  interface Gate {
    readonly promise: Promise<void>;
    resolve(): void;
  }

  function gate(): Gate {
    let resolve!: () => void;
    const promise = new Promise<void>((settle) => {
      resolve = settle;
    });
    return { promise, resolve };
  }

  /**
   * One document whose `getPage` answers the same cached promise — what pdf.js does per
   * `PDFDocumentProxy` — with a controllable `cleanup()` outcome and an optional render gate.
   */
  function fakeOwningDocument(
    sizes: Readonly<Record<number, PageSize>>,
    options: {
      cleanup?: () => boolean | void;
      render?: Promise<void>;
      getPage?: Promise<void>;
    } = {},
  ): CleanupWorld {
    const promises = new Map<number, Promise<ReaderPage>>();
    const objects = new Map<number, ReaderPage>();
    const cleanupCalls: number[] = [];
    const renderCalls: number[] = [];

    const document: ReaderDocument = {
      numPages: 4,
      getPage(page: number): Promise<ReaderPage> {
        const cached = promises.get(page);
        if (cached !== undefined) return cached;
        const created = (async () => {
          await options.getPage;
          const size = sizes[page];
          const pageObject: ReaderPage = {
            getViewport: ({ scale }) => ({ width: size.width * scale, height: size.height * scale }),
            render: () => {
              renderCalls.push(page);
              return { promise: options.render ?? Promise.resolve(), cancel: () => {} };
            },
            cleanup: () => {
              cleanupCalls.push(page);
              return options.cleanup?.();
            },
          };
          objects.set(page, pageObject);
          return pageObject;
        })();
        promises.set(page, created);
        return created;
      },
    };
    return { document, cleanupCalls, renderCalls, page: (page) => objects.get(page) };
  }

  function fakeCanvas(): HTMLCanvasElement {
    return { width: 0, height: 0, style: {} } as unknown as HTMLCanvasElement;
  }

  function sessionFor(
    document: ReaderDocument,
    generation: number,
    canvas: (page: number) => HTMLCanvasElement | undefined = () => fakeCanvas(),
  ): PageSession {
    return new PageSession({
      document,
      generation,
      pixelRatio: 1,
      canvas,
      changed: () => {},
    });
  }

  it("does not clean a page while another reader of the same document holds it", async () => {
    const world = fakeOwningDocument({ 1: { width: 300, height: 400 } });
    const first = sessionFor(world.document, 1);
    const second = sessionFor(world.document, 2);

    await first.measurePageSize(1);
    await second.measurePageSize(1);
    // pdf.js has one page proxy for this document, so both readers hold the same page.
    expect(await first.pageProxy(1)).toBe(await second.pageProxy(1));

    first.releaseMeasuredPages();
    await settle();
    expect(world.cleanupCalls).toEqual([]);

    second.releaseMeasuredPages();
    await settle();
    // The last owner is the one that cleans it, and only then.
    expect(world.cleanupCalls).toEqual([1]);
  });

  it("does not let a retired reader's late release clean the page a new reader renders", async () => {
    const renderGate = gate();
    const world = fakeOwningDocument(
      { 1: { width: 300, height: 400 } },
      { render: renderGate.promise },
    );
    const first = sessionFor(world.document, 1);
    const again = sessionFor(world.document, 3);

    first.renders.setView([1], 1); // A draws page 1; the render is still running
    await settle();
    const shared = await first.pageProxy(1);

    first.retire(); // replaced; its render is cancelled but not settled yet
    // The same document comes back, and a second reader draws the same page while the old one settles.
    again.renders.setView([1], 1);
    await settle();
    expect(await again.pageProxy(1)).toBe(shared);
    expect(world.cleanupCalls).toEqual([]);

    renderGate.resolve(); // the old render finally stops and gives up its claim
    await settle();
    // The new reader still owns the page, so the old release must not clean it under the new render.
    expect(world.cleanupCalls).toEqual([]);
    expect(world.renderCalls).toEqual([1, 1]);

    again.retire();
    await settle();
    expect(world.cleanupCalls).toEqual([1]);
  });

  it("does not clean a page while a settling render of the same document still owns it", async () => {
    const renderGate = gate();
    const world = fakeOwningDocument(
      { 1: { width: 300, height: 400 } },
      { render: renderGate.promise },
    );
    const drawing = sessionFor(world.document, 1);
    const measuring = sessionFor(world.document, 2, () => undefined);

    drawing.renders.setView([1], 1);
    await settle();
    await measuring.measurePageSize(1);
    measuring.releaseMeasuredPages();
    await settle();
    expect(world.cleanupCalls).toEqual([]);

    drawing.retire();
    renderGate.resolve();
    await settle();
    expect(world.cleanupCalls).toEqual([1]);
  });

  it("does not claim a cleanup pdf.js refused, and tries again for the next owner", async () => {
    let attempt = 0;
    const world = fakeOwningDocument(
      { 1: { width: 300, height: 400 } },
      {
        cleanup: () => {
          attempt += 1;
          return attempt > 1;
        },
      },
    );

    const first = sessionFor(world.document, 1);
    await first.measurePageSize(1);
    first.releaseMeasuredPages();
    await settle();

    // pdf.js refused; the page keeps its registration so a later owner can ask again.
    expect(world.cleanupCalls).toEqual([1]);
    // No trigger, no retry: a refusal is not retried on its own, so nothing spins.
    await settle();
    await settle();
    expect(world.cleanupCalls).toEqual([1]);

    const second = sessionFor(world.document, 2);
    await second.measurePageSize(1);
    second.releaseMeasuredPages();
    await settle();

    expect(world.cleanupCalls).toEqual([1, 1]);
    expect(attempt).toBe(2);
  });

  it("recovers a cleanup that throws, without an unhandled rejection and without losing the claim", async () => {
    const unhandled = watchUnhandledRejections();
    try {
      let attempts = 0;
      const world = fakeOwningDocument(
        { 1: { width: 300, height: 400 } },
        {
          cleanup: () => {
            attempts += 1;
            if (attempts === 1) throw new Error("the cleanup exploded");
            return true;
          },
        },
      );

      const first = sessionFor(world.document, 1);
      await first.measurePageSize(1);
      first.releaseMeasuredPages();
      await settle();

      // The attempt was made, and the throw was observed and contained: a cleanup that failed is not
      // claimed, and it must not surface as an unhandled rejection.
      expect(world.cleanupCalls).toEqual([1]);
      expect(unhandled.reasons).toEqual([]);

      const second = sessionFor(world.document, 2);
      await second.measurePageSize(1);
      second.releaseMeasuredPages();
      await settle();

      // The failed attempt kept the claim, so the next owner's release retries it and succeeds.
      expect(attempts).toBe(2);
      expect(unhandled.reasons).toEqual([]);
    } finally {
      unhandled.stop();
    }
  });

  it("keeps two documents' ownership apart", async () => {
    const worldA = fakeOwningDocument({ 1: { width: 300, height: 400 } });
    const worldB = fakeOwningDocument({ 1: { width: 600, height: 200 } });
    const readerA = sessionFor(worldA.document, 1);
    const readerB = sessionFor(worldB.document, 2);

    await readerA.measurePageSize(1);
    await readerB.measurePageSize(1);

    readerA.releaseMeasuredPages();
    await settle();
    expect(worldA.cleanupCalls).toEqual([1]);
    expect(worldB.cleanupCalls).toEqual([]);
  });
});

describe("the real pdf.js page cache (fixture-backed)", () => {
  /** The intrinsics pdf.js calls on paths a fixture load reaches; Node 22 lacks them. */
  function installMissingRuntimeShims(): () => void {
    const defined: Array<{ target: object; key: string }> = [];
    const define = (target: object, key: string, value: PropertyDescriptor) => {
      if (Object.getOwnPropertyDescriptor(target, key) !== undefined) return;
      Object.defineProperty(target, key, { configurable: true, writable: true, ...value });
      defined.push({ target, key });
    };

    define(Promise, "try", {
      value: (callback: (...args: unknown[]) => unknown, ...args: unknown[]) =>
        new Promise((resolve) => {
          resolve(callback(...args));
        }),
    });
    define(Uint8Array.prototype, "toHex", {
      value(this: Uint8Array): string {
        return Array.from(this, (byte) => byte.toString(16).padStart(2, "0")).join("");
      },
    });
    define(Map.prototype, "getOrInsertComputed", {
      value(this: Map<unknown, unknown>, key: unknown, compute: (key: unknown) => unknown): unknown {
        if (!this.has(key)) this.set(key, compute(key));
        return this.get(key);
      },
    });

    return () => {
      for (const { target, key } of defined.reverse()) Reflect.deleteProperty(target, key);
    };
  }

  /** Whether every intrinsic this suite shims is the runtime's own again. */
  function shimsAreRestored(): boolean {
    return (
      Object.getOwnPropertyDescriptor(Promise, "try") === undefined &&
      Object.getOwnPropertyDescriptor(Uint8Array.prototype, "toHex") === undefined &&
      Object.getOwnPropertyDescriptor(Map.prototype, "getOrInsertComputed") === undefined
    );
  }

  /**
   * Runs `run` with the shims installed and restores them whatever happens.
   *
   * The nesting is deliberate: teardown has its own `finally`, so a failed fixture read, a throwing
   * `getDocument`, a failed assertion OR a failed `destroy()` can never leave the shims on the shared
   * prototypes for every later test in this worker.
   */
  async function withPdfjsShims<T>(
    run: () => Promise<T>,
    teardown?: () => Promise<void>,
  ): Promise<T> {
    const restore = installMissingRuntimeShims();
    try {
      return await run();
    } finally {
      try {
        await teardown?.();
      } finally {
        restore();
      }
    }
  }

  it("restores the runtime shims when the work throws before any loader exists", async () => {
    const failure = new Error("the fixture could not be read");

    await expect(
      withPdfjsShims(async () => {
        throw failure;
      }),
    ).rejects.toThrow(/the fixture could not be read/);

    expect(shimsAreRestored()).toBe(true);
  });

  it("restores the runtime shims when the teardown itself fails", async () => {
    await expect(
      withPdfjsShims(
        async () => "loaded",
        async () => {
          throw new Error("the document could not be destroyed");
        },
      ),
    ).rejects.toThrow(/the document could not be destroyed/);

    expect(shimsAreRestored()).toBe(true);
  });

  it("hands one page proxy to two readers and cleans it only after the last owner", async () => {
    const fixtureFs = (await import(/* @vite-ignore */ "node:" + "fs/promises")) as {
      readFile(target: URL): Promise<Uint8Array>;
    };
    const { getDocument } = await import("pdfjs-dist");
    const fixture = new URL(
      "../../../crates/mirk-scan/tests/fixtures/with-info-metadata.pdf",
      import.meta.url,
    );
    let teardown: (() => Promise<void>) | null = null;

    await withPdfjsShims(
      async () => {
        const bytes = new Uint8Array(await fixtureFs.readFile(fixture));
        const loading = getDocument({ data: bytes, verbosity: 0 });
        // Registered BEFORE awaiting the load, so a failed load is still destroyed.
        teardown = async () => {
          await loading.destroy();
        };

        const pdf = await loading.promise;
        const first = await pdf.getPage(1);
        // The premise of the shared-ownership model: pdf.js answers the SAME proxy for one document.
        expect(await pdf.getPage(1)).toBe(first);
        const cleanup = vi.spyOn(first, "cleanup");

        const readerA = new PageSession({
          document: pdf,
          generation: 1,
          pixelRatio: 1,
          canvas: () => undefined,
          changed: () => {},
        });
        const readerB = new PageSession({
          document: pdf,
          generation: 2,
          pixelRatio: 1,
          canvas: () => undefined,
          changed: () => {},
        });

        expect(await readerA.pageProxy(1)).toBe(first);
        expect(await readerB.pageProxy(1)).toBe(first);

        await readerA.measurePageSize(1);
        await readerB.measurePageSize(1);
        readerA.releaseMeasuredPages();
        await settle();
        expect(cleanup).not.toHaveBeenCalled();

        readerB.releaseMeasuredPages();
        await settle();
        expect(cleanup).toHaveBeenCalledTimes(1);
        // pdf.js answers whether it really cleaned. A measured, never-rendered page can be cleaned at once,
        // so this pins the value the ownership code has to honour; a refusal is covered by the fakes.
        expect(cleanup.mock.results[0]?.value).toBe(true);
      },
      async () => {
        await teardown?.();
      },
    );

    // Whatever happened above, the shared prototypes are the runtime's own again.
    expect(shimsAreRestored()).toBe(true);
  });
});
describe("per-acquisition claims and coalesced cleanup", () => {
  interface Gate {
    readonly promise: Promise<void>;
    resolve(): void;
  }

  function gate(): Gate {
    let resolve!: () => void;
    const promise = new Promise<void>((settle) => {
      resolve = settle;
    });
    return { promise, resolve };
  }

  function fakeCanvas(): HTMLCanvasElement {
    return { width: 0, height: 0, style: {} } as unknown as HTMLCanvasElement;
  }

  function sessionFor(document: ReaderDocument, generation: number): PageSession {
    return new PageSession({
      document,
      generation,
      pixelRatio: 1,
      canvas: () => fakeCanvas(),
      changed: () => {},
    });
  }

  it("counts one claim per acquisition when a rejected request is also released by its render handle", async () => {
    const rejectRequest = gate();
    const holdRender = gate();
    let renderCount = 0;
    let requests = 0;
    const cleanupCalls: number[] = [];
    const pageObject: ReaderPage = {
      getViewport: ({ scale }) => ({ width: 300 * scale, height: 400 * scale }),
      render: () => {
        renderCount += 1;
        // The first real render finishes; the second stays live for the whole test.
        return { promise: renderCount === 1 ? Promise.resolve() : holdRender.promise, cancel: () => {} };
      },
      cleanup: () => {
        cleanupCalls.push(1);
        return true;
      },
    };
    // The first request for page 1 fails later; later requests answer the same page object. That is a legal
    // `ReaderDocument` (pdf.js itself caches the promise, which other suites cover), and it is what makes a
    // per-acquisition claim observable.
    const document: ReaderDocument = {
      numPages: 4,
      getPage(): Promise<ReaderPage> {
        requests += 1;
        if (requests === 1) {
          return (async () => {
            await rejectRequest.promise;
            throw new Error("the page request failed");
          })();
        }
        return Promise.resolve(pageObject);
      },
    };

    const failing = sessionFor(document, 1);
    const drawn = sessionFor(document, 2);
    const live = sessionFor(document, 3);

    failing.renders.setView([1], 1); // claim 1: request in flight
    await settle();
    drawn.renders.setView([1], 1); // claim 2: the same page object, drawn
    await settle();
    live.renders.setView([1], 1); // claim 3: the same page object, render still running
    await settle();

    // The failed request settles and its owner's render handle is released too. Both are the SAME claim:
    // releasing it twice would take a live reader's claim away.
    rejectRequest.resolve();
    await settle();

    drawn.renders.setView([], 1); // the drawn reader gives its page up while the last reader renders
    await settle();
    expect(cleanupCalls).toEqual([]);

    live.retire();
    holdRender.resolve();
    await settle();
    expect(cleanupCalls).toEqual([1]);
  });

  it("asks for one cleanup when several claims are released before the page request resolves", async () => {
    const resolvePage = gate();
    const cleanupCalls: number[] = [];
    const pageObject: ReaderPage = {
      getViewport: ({ scale }) => ({ width: 300 * scale, height: 400 * scale }),
      render: () => ({ promise: Promise.resolve(), cancel: () => {} }),
      cleanup: () => {
        cleanupCalls.push(1);
        return true;
      },
    };
    let requests = 0;
    const document: ReaderDocument = {
      numPages: 4,
      getPage(): Promise<ReaderPage> {
        requests += 1;
        // A fresh request per call, all resolving to the document's one page object once the gate opens.
        return (async () => {
          await resolvePage.promise;
          return pageObject;
        })();
      },
    };

    const first = sessionFor(document, 1);
    const second = sessionFor(document, 2);

    void first.measurePageSize(1).catch(() => {});
    first.retire(); // releases the pending claim: a zero-owner cleanup waits on the request
    void second.measurePageSize(1).catch(() => {});
    second.retire(); // a second zero-owner release before the request resolves

    resolvePage.resolve();
    await settle();

    // The two pending callbacks describe ONE page with no owners: it is cleaned once, not once per cycle.
    expect(cleanupCalls).toEqual([1]);
    expect(requests).toBe(2);
  });

  it("does not clean or forget a page a newer claim holds while an older cleanup is pending", async () => {
    const resolvePage = gate();
    const cleanupCalls: number[] = [];
    const pageObject: ReaderPage = {
      getViewport: ({ scale }) => ({ width: 300 * scale, height: 400 * scale }),
      render: () => ({ promise: Promise.resolve(), cancel: () => {} }),
      cleanup: () => {
        cleanupCalls.push(1);
        return true;
      },
    };
    const document: ReaderDocument = {
      numPages: 4,
      getPage(): Promise<ReaderPage> {
        return (async () => {
          await resolvePage.promise;
          return pageObject;
        })();
      },
    };

    const first = sessionFor(document, 1);
    const second = sessionFor(document, 2);

    void first.measurePageSize(1).catch(() => {});
    first.retire(); // zero owners; a cleanup is waiting on the request
    const newerClaim = second.measurePageSize(1); // a newer claim on the same document and page

    // The older zero-owner callback must see the new claim and leave the page alone.
    resolvePage.resolve();
    await newerClaim;
    await settle();
    expect(cleanupCalls).toEqual([]);

    second.retire();
    await settle();
    // The newer claim's release is the trigger that cleans it.
    expect(cleanupCalls).toEqual([1]);
  });
});
