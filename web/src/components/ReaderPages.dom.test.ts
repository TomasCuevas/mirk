import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import ReaderPages from "./ReaderPages.svelte";
import {
  ReaderPagesError,
  type ReaderDocument,
  type ReaderPage,
  type ReaderRenderOptions,
} from "../lib/reader-pages";

/**
 * The continuous page view, in a simulated browser.
 *
 * The arithmetic — which pages a window names, what a zoom does to it, when a canvas is released — is
 * pinned without a DOM in `web/src/lib/reader-pages.test.ts`. What only a mounted component can answer
 * is asked here: does a canvas exist for exactly those pages, does a scroll cancel the render that left
 * the window, does a zoom press redraw at the new size, and does the interface state its loading and its
 * failures instead of showing an empty page.
 *
 * The document is a fake with pdf.js's three behaviours, so nothing here loads a PDF, starts a worker or
 * paints anything: a test that needed a real PDF would be testing pdf.js, not this component.
 */

const SIZE = { width: 300, height: 400 };

interface RenderCall {
  readonly page: number;
  /** The scale pdf.js was asked to draw at, after the device pixel ratio. */
  readonly scale: number;
  readonly client: HTMLCanvasElement | null | undefined;
  readonly canvasWidth: number;
  readonly canvasHeight: number;
  cancelled: boolean;
}

interface WaitingRender {
  readonly page: number;
  readonly finish: (error?: Error) => void;
}

interface FakeWorld {
  readonly document: ReaderDocument;
  readonly renders: RenderCall[];
  readonly cleanups: number[];
  /** Every page the component asked for, in order: measurement and rendering both. */
  readonly asked: number[];
  /** The most renders that were running at the same time. */
  mostActive(): number;
  /** Pages that were asked to draw while a render for the same page was still running. */
  readonly overlaps: number[];
  /** How many renders are still waiting for the test to end them. */
  waiting(): number;
  /** Ends every render that is waiting. */
  finish(): void;
  /** Ends the first waiting render of one page. */
  finishPage(page: number): void;
}

/**
 * A document of equal pages, with the three behaviours the reader uses.
 *
 * `pending` keeps every render unfinished until the test ends it, which is how a cancellation can be
 * observed: `cancel()` only marks the render, exactly as pdf.js does, because a render task that is asked
 * to stop keeps running for a while and settles its promise later. `failOnce` makes one page refuse its
 * first draw; `badSize` makes one page report a zero width; `gate` holds every `getPage` until it is
 * opened, so the measuring state is observable.
 *
 * It records the two facts a canvas handover can violate: how many renders ran at once, and whether a
 * page was asked to draw while its previous render was still running (a reused canvas).
 */
function fakeDocument(
  options: {
    pages?: number;
    pending?: boolean;
    failOnce?: number;
    badSize?: number;
    gate?: Promise<void>;
    /** Only pages at or after this number wait for `gate`; earlier ones answer at once. */
    gateFrom?: number;
    /** One page's real size; every other page is the default 300x400 portrait. */
    sizeFor?: (page: number) => { width: number; height: number };
  } = {},
): FakeWorld {
  const pageCount = options.pages ?? 4;
  const renders: RenderCall[] = [];
  const cleanups: number[] = [];
  const asked: number[] = [];
  const waiting: WaitingRender[] = [];
  const overlaps: number[] = [];
  const activePages = new Set<number>();
  const attempts = new Map<number, number>();
  let active = 0;
  let mostActive = 0;

  const sizeOf = (pageNumber: number): { width: number; height: number } => {
    if (pageNumber === options.badSize) return { width: 0, height: SIZE.height };
    return options.sizeFor?.(pageNumber) ?? SIZE;
  };

  // pdf.js answers the SAME page object for repeated `getPage` calls; a fake that returns a new one per
  // call cannot see an ownership bug where two documents' pages for one number are confused.
  const pageObjects = new Map<number, ReaderPage>();

  const pageFor = (pageNumber: number): ReaderPage => {
    const cached = pageObjects.get(pageNumber);
    if (cached !== undefined) return cached;

    const page: ReaderPage = {
      getViewport: ({ scale }) => {
        const size = sizeOf(pageNumber);
        return { width: size.width * scale, height: size.height * scale };
      },
      render: (request: ReaderRenderOptions) => {
        const attempt = (attempts.get(pageNumber) ?? 0) + 1;
        attempts.set(pageNumber, attempt);
        if (activePages.has(pageNumber)) overlaps.push(pageNumber);
        activePages.add(pageNumber);
        active += 1;
        mostActive = Math.max(mostActive, active);

        const call: RenderCall = {
          page: pageNumber,
          scale: request.viewport.width / sizeOf(pageNumber).width,
          client: request.canvas,
          canvasWidth: request.canvas?.width ?? 0,
          canvasHeight: request.canvas?.height ?? 0,
          cancelled: false,
        };
        renders.push(call);

        let settleDraw: ((error: Error | null) => void) | null = null;
        const promise = new Promise<void>((resolve, reject) => {
          settleDraw = (error) => (error === null ? resolve() : reject(error));
        });
        let finished = false;
        const finish = (error?: Error): void => {
          if (finished) return;
          finished = true;
          active -= 1;
          activePages.delete(pageNumber);
          settleDraw?.(error ?? null);
        };

        if (options.failOnce === pageNumber && attempt === 1) {
          finish(new ReaderPagesError("the page stream is broken"));
        } else if (options.pending === true) {
          waiting.push({ page: pageNumber, finish });
        } else {
          // A real render settles on a later turn, never inside `render()`. Keeping that shape here is what
          // stops this suite from proving anything with a synchronously-finished fake.
          queueMicrotask(() => finish());
        }

        return {
          promise,
          // Only asks the render to stop. The promise settles when the render actually ends.
          cancel: () => {
            call.cancelled = true;
          },
        };
      },
      cleanup: () => {
        cleanups.push(pageNumber);
      },
    };

    pageObjects.set(pageNumber, page);
    return page;
  };

  return {
    document: {
      numPages: pageCount,
      getPage: async (pageNumber: number) => {
        asked.push(pageNumber);
        if (options.gateFrom === undefined || pageNumber >= options.gateFrom) await options.gate;
        return pageFor(pageNumber);
      },
    },
    renders,
    cleanups,
    asked,
    mostActive: () => mostActive,
    overlaps,
    waiting: () => waiting.length,
    finish: () => {
      for (const entry of waiting.splice(0)) entry.finish();
    },
    finishPage: (page: number) => {
      const index = waiting.findIndex((entry) => entry.page === page);
      if (index === -1) throw new Error(`page ${page} has no render waiting`);
      const [entry] = waiting.splice(index, 1);
      entry.finish();
    },
  };
}

let component: ReturnType<typeof mount> | null = null;

afterEach(() => {
  if (component !== null) {
    unmount(component);
    component = null;
  }
  window.document.body.innerHTML = "";
});

/**
 * Mounts the reader with a one-device-pixel ratio and no overscan, so the arithmetic in the assertions is
 * the arithmetic of the test and not of the machine it runs on.
 */
function mountReader(
  document: ReaderDocument,
  props: {
    maxWindowPages?: number;
    overscan?: number;
    onPageChange?: (page: number) => void;
    jumpToPage?: number | null;
  } = {},
): HTMLElement {
  const target = window.document.createElement("div");
  window.document.body.appendChild(target);
  component = mount(ReaderPages, {
    target,
    props: {
      document,
      pixelRatio: 1,
      overscan: props.overscan ?? 0,
      maxWindowPages: props.maxWindowPages ?? 2,
      onPageChange: props.onPageChange,
      jumpToPage: props.jumpToPage ?? null,
      },
  });
  flushSync();
  return target;
}

/** Lets the measuring, the layout and the render bookkeeping settle, then flushes the DOM. */
async function settle(): Promise<void> {
  for (let turn = 0; turn < 8; turn += 1) {
    await Promise.resolve();
    flushSync();
  }
  await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
}

/** Ends every waiting render, including the ones the model starts as a result, until nothing waits. */
async function finishAll(world: FakeWorld): Promise<void> {
  for (let round = 0; round < 8; round += 1) {
    if (world.waiting() === 0) return;
    world.finish();
    await settle();
  }
}

/**
 * Gives the scroller a viewport height and a scroll offset, then reports a scroll.
 *
 * happy-dom lays nothing out, so `clientHeight` and `scrollTop` are 0 unless the test says otherwise.
 * Both are defined on the element itself, which is also what lets the zoom test read back the offset the
 * component restored.
 */
function scrollTo(target: HTMLElement, scrollTop: number, viewportHeight = 400): HTMLElement {
  const scroller = target.querySelector<HTMLElement>(".scroller");
  if (scroller === null) throw new Error("the reader has no scroller");
  Object.defineProperty(scroller, "clientHeight", { configurable: true, value: viewportHeight });
  Object.defineProperty(scroller, "scrollTop", {
    configurable: true,
    writable: true,
    value: scrollTop,
  });
  scroller.dispatchEvent(new Event("scroll"));
  flushSync();
  return scroller;
}

function pagesOnScreen(target: HTMLElement): string[] {
  return Array.from(target.querySelectorAll("[data-page]")).map(
    (slot) => slot.getAttribute("data-page") ?? "",
  );
}

function slotOf(target: HTMLElement, page: number): HTMLElement | null {
  return target.querySelector<HTMLElement>(`[data-page="${page}"]`);
}

function button(target: HTMLElement, label: string): HTMLButtonElement {
  const found = target.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  if (found === null) throw new Error(`there is no ${label} control`);
  return found;
}

describe("the continuous page view", () => {
  it("says it is measuring before it can draw, then draws the first page", async () => {
    let open: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      open = resolve;
    });
    const world = fakeDocument({ gate });

    const target = mountReader(world.document);

    expect(target.querySelector("canvas")).toBeNull();
    expect(target.textContent).toContain("Measuring");
    expect(world.renders).toEqual([]);

    open();
    await settle();

    expect(target.querySelectorAll("canvas")).toHaveLength(1);
    expect(pagesOnScreen(target)).toEqual(["1"]);
    expect(world.renders[0]).toMatchObject({ page: 1, scale: 1, canvasWidth: 300, canvasHeight: 400 });
  });

  it("reports the first page on screen as the reader scrolls, so the margin can follow", async () => {
    // The margin's number is this report. If it stopped arriving at a scroll, the reader would be told they
    // are on page one for the whole book — which is what a number that never changes looked like.
    const world = fakeDocument();
    const seen: number[] = [];
    const target = mountReader(world.document, { onPageChange: (page) => seen.push(page) });
    await settle();

    // Nothing is reported before the reader scrolls: the screen already knows the page it opened at.
    expect(seen).toEqual([]);

    scrollTo(target, 460, 400);
    await settle();

    const first = Number(pagesOnScreen(target)[0]);
    expect(seen.at(-1)).toBe(first - 1);
    expect(seen.at(-1)).toBeGreaterThan(0);
  });

  it("brings a page to the top of the viewport when the screen asks for it", async () => {
    // Jumping is the only way to move without scrolling, and the control that asks for it sends a
    // zero-based page here: page three is index two.
    const world = fakeDocument();
    const target = mountReader(world.document, { jumpToPage: 2 });
    await settle();

    expect(pagesOnScreen(target)[0]).toBe("3");
  });

  it("names the reader region and every page for assistive technology", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();

    expect(target.querySelector(".reader")?.getAttribute("aria-label")).toBe("Document pages");
    expect(target.querySelector(".scroller")?.getAttribute("role")).toBe("region");
    const canvas = target.querySelector("canvas");
    expect(canvas?.getAttribute("role")).toBe("img");
    expect(canvas?.getAttribute("aria-label")).toBe("Page 1");
  });

  it("keeps every page the viewport touches, and queues the draws past the render bound", async () => {
    const world = fakeDocument({ pending: true });

    const target = mountReader(world.document, { maxWindowPages: 2 });
    await settle();
    // The whole document fits the tall viewport; the page budget may not hide any of it.
    scrollTo(target, 0, 1672);
    await settle();

    expect(pagesOnScreen(target)).toEqual(["1", "2", "3", "4"]);
    // Only the render bound runs at once; the rest wait their turn instead of disappearing.
    expect(world.renders.map((call) => call.page)).toEqual([1, 2]);
    expect(world.mostActive()).toBeLessThanOrEqual(2);
    expect(target.textContent).toContain("Waiting to draw page 3");
    expect(target.textContent).toContain("Pages 1–4 of 4");

    await finishAll(world);

    expect(world.renders.map((call) => call.page)).toEqual([1, 2, 3, 4]);
    expect(world.mostActive()).toBeLessThanOrEqual(2);
    expect(world.overlaps).toEqual([]);
    expect(target.textContent).not.toContain("Waiting to draw");
    expect(target.querySelectorAll(".page-loading")).toHaveLength(0);
  });

  it("starts the next page only after the cancelled render has really stopped", async () => {
    const world = fakeDocument({ pending: true });

    const target = mountReader(world.document);
    await settle();
    scrollTo(target, 0, 400);
    await settle();
    expect(world.renders.map((call) => call.page)).toEqual([1]);

    scrollTo(target, 900, 400);
    await settle();

    // Pages 3 and 4 are the window now. Page 1's render was cancelled but has not stopped, so it still
    // holds a slot: only page 3 may start, and page 4 waits its turn.
    expect(pagesOnScreen(target)).toEqual(["3", "4"]);
    expect(world.renders.map((call) => call.page)).toEqual([1, 3]);
    expect(world.renders[0].cancelled).toBe(true);
    expect(target.textContent).toContain("Waiting to draw page 4");

    world.finishPage(1);
    await settle();

    expect(world.renders.map((call) => call.page)).toEqual([1, 3, 4]);
    expect(world.mostActive()).toBeLessThanOrEqual(2);
    expect(world.overlaps).toEqual([]);
  });

  it("does not reuse a page's canvas while its previous render is still settling", async () => {
    const world = fakeDocument({ pending: true });

    const target = mountReader(world.document);
    await settle();
    expect(world.renders).toHaveLength(1);

    button(target, "Zoom in").click();
    await settle();

    // The old render is cancelled but still running: the same canvas must not be drawn into again yet.
    expect(world.renders).toHaveLength(1);
    expect(world.renders[0].cancelled).toBe(true);

    world.finishPage(1);
    await settle();

    expect(world.renders).toHaveLength(2);
    expect(world.renders[1]).toMatchObject({ page: 1, scale: 1.25, canvasWidth: 375 });
    // The same canvas element is reused, but only after the old render stopped.
    expect(world.renders[1].client).toBe(world.renders[0].client);
    expect(world.overlaps).toEqual([]);
    expect(world.mostActive()).toBe(1);
  });

  it("releases the canvas and the page resources the evicted page no longer needs", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();
    const firstCanvas = target.querySelector<HTMLCanvasElement>('canvas[aria-label="Page 1"]');
    expect(firstCanvas).not.toBeNull();

    scrollTo(target, 900, 400);
    await settle();

    expect(pagesOnScreen(target)).toEqual(["3", "4"]);
    expect(world.cleanups).toContain(1);
    // The canvas left the document with its slot, so the buffer went with it.
    expect(target.querySelector('canvas[aria-label="Page 1"]')).toBeNull();
  });

  it("zooms with accessible controls and redraws the pages at the new scale", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();
    expect(button(target, "Zoom in").disabled).toBe(false);
    expect(button(target, "Zoom out").disabled).toBe(false);
    expect(target.querySelector('[aria-label="Zoom level"]')?.textContent?.trim()).toBe("100%");

    button(target, "Zoom in").click();
    await settle();

    expect(world.renders.map((call) => [call.page, call.scale])).toEqual([
      [1, 1],
      [1, 1.25],
    ]);
    expect(world.renders[1].canvasWidth).toBe(375);
    expect(target.querySelector('[aria-label="Zoom level"]')?.textContent?.trim()).toBe("125%");

    button(target, "Zoom out").click();
    button(target, "Zoom out").click();
    await settle();

    // 125% back to 100%, then one more step out: 80%.
    expect(world.renders.at(-1)).toMatchObject({ page: 1, scale: 0.8, canvasWidth: 240 });
    expect(target.querySelector('[aria-label="Zoom level"]')?.textContent?.trim()).toBe("80%");
  });

  it("stops at the zoom bounds instead of offering a press that does nothing", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();

    for (let press = 0; press < 12; press += 1) button(target, "Zoom out").click();
    await settle();

    expect(target.querySelector('[aria-label="Zoom level"]')?.textContent?.trim()).toBe("50%");
    expect(button(target, "Zoom out").disabled).toBe(true);

    for (let press = 0; press < 20; press += 1) button(target, "Zoom in").click();
    await settle();

    expect(target.querySelector('[aria-label="Zoom level"]')?.textContent?.trim()).toBe("400%");
    expect(button(target, "Zoom in").disabled).toBe(true);
  });

  it("keeps the reading position when the zoom changes", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();
    const scroller = scrollTo(target, 624, 400);
    await settle();

    button(target, "Zoom in").click();
    await settle();

    // The stack's 20 px padding is part of the layout, so page 2 runs 444–844 at 100% and 624 is 45% down
    // it. At 125% page 2 starts at 544 and is 500 CSS pixels tall: 544 + 0.45 × 500 = 769.
    expect(scroller.scrollTop).toBe(769);
  });

  it("reports a page that could not be drawn and tries it again on request", async () => {
    const world = fakeDocument({ failOnce: 1 });

    const target = mountReader(world.document);
    await settle();

    const alert = target.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("the page stream is broken");
    // The slot keeps its canvas — it has the page's size — but nothing was drawn into it, and the scrim
    // says so instead of presenting the paper as a blank page.
    expect(target.querySelector(".page-failure")).not.toBeNull();
    expect(target.textContent).toContain("Page 1");
    // A failed render's resources are released instead of kept for a page that drew nothing.
    expect(world.cleanups).toEqual([1]);

    button(target, "Retry page 1").click();
    await settle();

    expect(target.querySelector('[role="alert"]')).toBeNull();
    expect(target.querySelector(".page-failure")).toBeNull();
    expect(world.renders.map((call) => call.page)).toEqual([1, 1]);
    expect(target.querySelector('canvas[aria-label="Page 1"]')).not.toBeNull();
  });

  it("refuses to lay out a page that reports no size, and names it", async () => {
    const world = fakeDocument({ badSize: 2 });

    const target = mountReader(world.document);
    await settle();

    expect(target.querySelector('[role="alert"]')?.textContent).toContain("page 2");
    expect(target.querySelectorAll("canvas")).toHaveLength(0);
  });

  it("lays an unmeasured page out as a placeholder and corrects it when the page is acquired", async () => {
    // 514 pages: the tail is past the measurement cap, and page 513 is landscape.
    const world = fakeDocument({
      pages: 514,
      sizeFor: (page) => (page === 513 ? { width: 600, height: 200 } : SIZE),
    });

    const target = mountReader(world.document);
    await settle();
    expect(target.querySelector(".estimate")?.textContent).toContain("Pages after 512");

    // Page 513's estimated box: 20 px of stack padding and 512 pages of 400 px with 24 px gaps.
    const page513Top = 20 + 512 * (400 + 24);
    const scroller = scrollTo(target, page513Top, 400);
    await settle();

    // Drawn at its real size: a 600x200 page is never stretched into the 300x400 estimate, and the layout
    // is corrected rather than the picture.
    expect(slotOf(target, 513)?.style.width).toBe("600px");
    expect(slotOf(target, 513)?.style.height).toBe("200px");
    // The page-513 render used the page's real shape, and the corrected (shorter) box brings 514 into view.
    const page513Render = [...world.renders].reverse().find((call) => call.page === 513);
    expect(page513Render).toMatchObject({
      page: 513,
      canvasWidth: 600,
      canvasHeight: 200,
    });
    expect(world.overlaps).toEqual([]);

    // The correction did not move the reader: the anchor was page 513's top and it is still there.
    expect(scroller.scrollTop).toBe(page513Top);

    // Page 514 now starts after the corrected 200 px, so its box is at the corrected offset.
    scrollTo(target, page513Top + 350, 400);
    await settle();
    expect(slotOf(target, 514)?.style.top).toBe(`${page513Top + 200 + 24}px`);
  });

  it("announces the pages in view, not the overscan pages it keeps ready", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document, { overscan: 600 });
    await settle();
    scrollTo(target, 500, 300);
    await settle();

    // Only page 2 is on screen; page 1 is drawn ahead of it and must not be announced as visible.
    expect(target.textContent).toContain("Page 2 of 4");
    expect(target.textContent).not.toContain("Pages 1–2 of 4");
  });

  it("measures the stack against the scroller, so a note above it does not shift the viewport", async () => {
    const world = fakeDocument({ pages: 520 });

    const target = mountReader(world.document);
    await settle();
    expect(target.querySelector(".estimate")).not.toBeNull();

    // The note is above the stack and its height is whatever the text needs. 48 px stands in for it.
    const stack = target.querySelector<HTMLElement>(".stack");
    if (stack === null) throw new Error("the reader has no stack");
    Object.defineProperty(stack, "offsetTop", { configurable: true, value: 48 });

    // 1300 px of scroller is 1252 px of stack, which is pages 3 and 4. Ignoring the note would say page 4.
    scrollTo(target, 1300, 400);
    await settle();

    expect(target.textContent).toContain("Pages 3–4 of 520");
  });

  it("releases the canvas its own render drew into, even after the slot left and came back", async () => {
    const world = fakeDocument({ pending: true });

    const target = mountReader(world.document);
    await settle();
    scrollTo(target, 0, 400);
    await settle();
    const oldCanvas = target.querySelector<HTMLCanvasElement>('canvas[aria-label="Page 1"]');
    if (oldCanvas === null) throw new Error("page 1 has no canvas");
    expect(oldCanvas.width).toBe(300);

    scrollTo(target, 900, 400); // page 1 leaves; its render is cancelled but still running
    await settle();
    scrollTo(target, 0, 400); // page 1 comes back with a NEW canvas, while the old render still holds it
    await settle();

    const newCanvas = target.querySelector<HTMLCanvasElement>('canvas[aria-label="Page 1"]');
    if (newCanvas === null) throw new Error("page 1 came back without a canvas");
    expect(newCanvas).not.toBe(oldCanvas);
    // Not drawn into yet: the old render for the page has not settled, so this is still a fresh element.
    expect(newCanvas.width).toBe(300);
    expect(newCanvas.height).toBe(150);

    world.finishPage(1);
    await settle();

    // The release went to the canvas that render used (now detached); the slot's new canvas is untouched.
    expect(oldCanvas.width).toBe(0);
    expect(oldCanvas.height).toBe(0);
    expect(world.renders.at(-1)?.client).toBe(newCanvas);
    expect(newCanvas.width).toBe(300);
    expect(newCanvas.height).toBe(400);
  });

  it("does not clean a page another reader of the same document is using", async () => {
    // One document instance, two readers: pdf.js hands both the same page proxies for it.
    const world = fakeDocument();

    const targetA = window.document.createElement("div");
    const targetB = window.document.createElement("div");
    window.document.body.append(targetA, targetB);
    const readerA = mount(ReaderPages, {
      target: targetA,
      props: { document: world.document, pixelRatio: 1, overscan: 0, maxWindowPages: 2 },
    });
    const readerB = mount(ReaderPages, {
      target: targetB,
      props: { document: world.document, pixelRatio: 1, overscan: 0, maxWindowPages: 2 },
    });
    flushSync();
    await settle();

    expect(world.renders.filter((call) => call.page === 1)).toHaveLength(2);

    unmount(readerA);
    flushSync();
    await settle();
    // Reader B still owns every page of this document, so nothing may be cleaned yet.
    expect(world.cleanups).toEqual([]);

    unmount(readerB);
    flushSync();
    await settle();
    // The last owner releases each page, once.
    expect([...world.cleanups].sort((left, right) => left - right)).toEqual([1, 2, 3, 4]);
  });

  it("leaves the reader's own scroll alone when a zoom restore is still pending", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();
    const scroller = scrollTo(target, 624, 400);
    await settle();

    button(target, "Zoom in").click();
    // The reader scrolls before the deferred DOM write runs; their position is the one that must win.
    scrollTo(target, 900, 400);
    await settle();

    expect(scroller.scrollTop).toBe(900);
  });

  it("applies only the newest of two quick zoom restores", async () => {
    const world = fakeDocument();

    const target = mountReader(world.document);
    await settle();
    const scroller = scrollTo(target, 624, 400);
    await settle();

    button(target, "Zoom in").click();
    button(target, "Zoom in").click();
    await settle();

    // 100% → 125% → 156.25%, each anchored on page 2 at the same 45% of it. At 156.25% page 2 starts at
    // 669 and is 625 tall, so the position is 669 + 0.45 × 625 = 950.25 — not the first zoom's 769.
    expect(scroller.scrollTop).toBeCloseTo(950.25, 6);
    expect(target.querySelector('[aria-label="Zoom level"]')?.textContent?.trim()).toBe("156%");
  });

  it("stops measuring after the reader leaves and releases the pages it acquired", async () => {
    let open: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      open = resolve;
    });
    const world = fakeDocument({ pages: 20, gate, gateFrom: 2 });

    const target = mountReader(world.document);
    await settle();
    expect(world.asked).toContain(1);
    expect(world.asked).toContain(2);
    expect(world.asked).not.toContain(3);

    if (component !== null) unmount(component);
    component = null;
    flushSync();

    const askedAtTeardown = [...world.asked];
    open();
    await settle();

    // The pass stopped where it was and did not ask for page 3; the page it had in flight is released.
    expect(world.asked).toEqual(askedAtTeardown);
    expect(world.asked).not.toContain(3);
    expect(world.cleanups).toContain(2);
    expect(target.querySelector("canvas")).toBeNull();
  });

  it("cancels the render it was waiting for and releases it once it stops, even after the reader left", async () => {
    const world = fakeDocument({ pending: true });

    const target = mountReader(world.document);
    await settle();
    expect(world.renders).toHaveLength(1);

    if (component !== null) unmount(component);
    component = null;
    flushSync();

    expect(world.renders[0].cancelled).toBe(true);
    expect(target.querySelector("canvas")).toBeNull();
    // Nothing may be released while the render is still running.
    expect(world.cleanups).toEqual([]);

    world.finishPage(1);
    await settle();

    // The render's page goes back when the cancelled drawing actually stops, and the pages the size
    // measurement acquired are released too: disposal leaked neither.
    expect(world.cleanups).toContain(1);
    expect([...world.cleanups].sort((left, right) => left - right)).toEqual([1, 2, 3, 4]);
    expect(world.renders).toHaveLength(1);
  });
});
