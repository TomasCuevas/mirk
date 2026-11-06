<script lang="ts">
  // The continuous page view: a reusable reader surface over a document pdf.js has ALREADY loaded.
  //
  // It opens nothing. The document arrives as a prop (`ReaderDocument`, which `PDFDocumentProxy`
  // satisfies), so this component cannot name a file, a document id, a native reading session or a
  // transport, and it
  // registers no command. Its only consumer today is the DEBUG-ONLY fixture page
  // (`web/src/components/ReaderFixture.svelte`), whose document comes from the embedded fixture through
  // the bounded fixture commands; production wiring waits for the real document transport, which is
  // unresolved (T1). Nothing in the application shell imports this file.
  //
  // What it owns is the reading surface: the stack of pages, which of them exist as canvases, the zoom,
  // and the honest state of each page. The decisions behind that — layout, the visible window, the render
  // queue, eviction and the canvas bounds — live in `../lib/reader-pages` and are tested there without a
  // DOM. This file is the thin part: it applies those decisions to canvases and says what is happening
  // while it does.
  //
  // DOCUMENT OWNERSHIP. A `PageSession` owns one document's proxy cache, measured pages and render set. It
  // is replaced, never mutated, when the document is: old pending acquisitions, cancellations and
  // cleanups can only reach the old session's pages, canvases and layout, because they captured them.
  // Canvases are keyed by session generation, so a replaced document's slots are new elements too.
  import { onMount, tick, untrack } from "svelte";
  import {
    DEFAULT_OVERSCAN,
    DEFAULT_WINDOW_PAGES,
    MeasurementStopped,
    PAGE_GAP,
    PAGE_STACK_PADDING,
    PageSession,
    ScrollRestoreGuard,
    anchorAt,
    clampScale,
    offsetForAnchor,
    pageLayout,
    pageWindow,
    readPageSizes,
    withPageSize,
    zoomScale,
    type PageBox,
    type PageRenderSnapshot,
    type PageSize,
    type ReaderDocument,
    type ScrollAnchor,
  } from "../lib/reader-pages";

  let {
    document: pdf,
    initialScale = 1,
    overscan = DEFAULT_OVERSCAN,
    maxWindowPages = DEFAULT_WINDOW_PAGES,
    initialPage = 0,
    currentPage = $bindable(0),
    jumpToPage = null,
    onPageChange,
    onScaleChange,
    pixelRatio = typeof globalThis.devicePixelRatio === "number" ? globalThis.devicePixelRatio : 1,
  }: {
    /** A document pdf.js has already loaded. The reader never loads or opens one itself. */
    document: ReaderDocument;
    /** The zoom the reader starts at. */
    initialScale?: number;
    /** How far above and below the viewport a page is still worth drawing. */
    overscan?: number;
    /**
     * The budget for pages the viewport does NOT touch.
     *
     * It is deliberately not a cap on how many pages may exist: every page the viewport touches is kept,
     * so a window showing more pages than this still shows all of them and the render queue draws them a
     * couple at a time. Hiding a visible page to honour a canvas count would leave a hole where a page
     * should be; the memory bound is per canvas and per in-flight render instead.
     */
    maxWindowPages?: number;
    /** The screen's device pixel ratio; one page's buffer is capped rather than inflated by it. */
    pixelRatio?: number;
    /**
     * The page to open at, zero-based, exactly as `progress.page` is stored (S4).
     *
     * `0` is the first page and the default. The offset is applied once, when the layout first exists, and
     * it is an estimate for any page whose real size has not been measured yet — the page view corrects
     * sizes as it acquires them, exactly as it does while scrolling.
     */
    initialPage?: number;
    /**
     * The first page on screen, zero-based, bound both ways.
     *
     * The screen draws this number in its margin, and a binding tells it directly instead of through a
     * callback it might not re-render from.
     */
    currentPage?: number;
    /** A page to bring to the top of the viewport, zero-based; `null` when nothing was asked. */
    jumpToPage?: number | null;
    /**
     * The zoom this page view is drawing at, reported whenever it changes.
     *
     * The controls live in the reader's one bar (`ReaderScreen`), so the level has to travel to it; the
     * scale itself stays here, because the layout is derived from it.
     */
    onScaleChange?: (scale: number) => void;
    /**
     * Called with the first page on screen, zero-based, whenever that page changes.
     *
     * The first page of the ones the reader can see, not of the overscan band around them: a page kept
     * ready off screen is not a page anybody read. Touched during a zoom or a size correction that did not
     * move the reader, it reports the same page again and the caller decides whether that is news.
     */
    onPageChange?: (page: number) => void;
  } = $props();

  /** The zoom, clamped so a bad initial value cannot produce a zero-height stack. */
  let scale = $state(untrack(() => clampScale(initialScale)));

  /** Tells the screen the level it should draw in its bar. */
  $effect(() => {
    onScaleChange?.(scale);
  });
  /** The pages' sizes: measured, or `null` for a page nothing has acquired yet. */
  let sizes = $state<readonly (PageSize | null)[] | null>(null);
  /** How many leading pages the measurement pass read; fewer than the document has means the rest are unknown. */
  let measured = $state(0);
  /** Why the document could not be laid out at all, if it could not. */
  let loadFailure = $state<string | null>(null);
  /**
   * The scroll offset in the STACK's coordinates.
   *
   * The scroller can have content above the stack — the estimation note — and its height depends on the
   * text, so the two origins differ by whatever the note takes. The offset is kept relative to the stack
   * (see `stackOrigin`) so the window, the zoom anchor and the sizes correction all speak the layout's
   * coordinates and no height is ever hardcoded.
   */
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let viewportWidth = $state(0);

  let scroller: HTMLDivElement | undefined = $state();
  let stack: HTMLDivElement | undefined = $state();

  /**
   * The current document's session: its proxy cache, its measurements and its render set. A replacement
   * assigns a new session, so everything an old document still has in flight can only reach the old one.
   */
  let session = $state<PageSession | null>(null);

  /**
   * The DOM canvases, keyed by session generation and page.
   *
   * The generation in the key is what stops a new document from reusing the previous one's canvas
   * elements: the old slots are destroyed with their generation and the new document gets new ones, so an
   * old draw's release can only ever zero a detached element.
   */
  const canvases = new Map<string, HTMLCanvasElement>();
  /** Guards deferred scroll writes: a newer restore, a scroll or a replacement makes one stale. */
  const restores = new ScrollRestoreGuard();
  let generation = 0;
  /** Set false when the route leaves, so a late settle cannot report into a destroyed component. */
  let live = true;

  let snapshot = $state<PageRenderSnapshot>(
    untrack(() => ({ scale, window: [], drawn: [], drawing: [], failed: [] })),
  );

  const layout = $derived(
    sizes === null
      ? null
      : pageLayout(sizes, { scale, gap: PAGE_GAP, padding: PAGE_STACK_PADDING }),
  );
  const plan = $derived(
    layout === null
      ? null
      : pageWindow(layout, { scrollTop, viewportHeight, overscan, maxPages: maxWindowPages }),
  );
  const boxes = $derived<readonly PageBox[]>(
    layout === null || plan === null ? [] : layout.boxes.slice(plan.start, plan.end),
  );
  const busy = $derived(snapshot.drawing.length > 0);
  const sessionGeneration = $derived(session?.generation ?? 0);

  const position = $derived.by(() => {
    if (loadFailure !== null) return "No pages";
    if (plan === null || plan.visible.length === 0) return "Measuring…";
    // The pages the reader can see, not the pages kept ready in the overscan band around them.
    const first = plan.visible[0];
    const last = plan.visible[plan.visible.length - 1];
    return first === last
      ? `Page ${first} of ${pdf.numPages}`
      : `Pages ${first}–${last} of ${pdf.numPages}`;
  });

  /**
   * Opens at the page the store remembers, once.
   *
   * It waits for the first layout, because there is no scroll offset to give before the stack has a
   * height, and it runs once: a resume that reapplied itself on every size correction would fight the
   * reader for the scroll position.
   */
  let resumed = false;
  $effect(() => {
    if (resumed || layout === null || initialPage <= 0) return;
    resumed = true;
    scheduleRestore(offsetForAnchor(layout, { page: initialPage + 1, ratio: 0 }));
  });

  /**
   * Reports the first page on screen, zero based, when it changes.
   *
   * It is called from the scroll event and from nowhere else. That is deliberate and it is the fix for a
   * bug that appeared and disappeared between launches: a `$effect` that reacts to the plan and writes
   * state the parent owns is a flush-phase hazard. When Svelte refuses that write, the page reaches
   * neither the margin nor the store — while a test that drives the scroll handler directly still passes,
   * because an event handler is exactly where the write is allowed.
   *
   * Nothing is reported before the reader scrolls: the screen already knows the page it opened at, from
   * the store, and reporting it again would only echo it back.
   */
  let reported = -1;

  function reportFirstVisible(onScreen: readonly number[] | undefined): void {
    const first = onScreen?.[0];
    if (first === undefined || first === reported) return;
    reported = first;
    currentPage = first - 1;
    onPageChange?.(first - 1);
  }

  /**
   * Fits the page to the column the first time the sizes and the viewport are both known.
   *
   * A page drawn at 100 % is its printed size: 612 pixels for an A4. In a narrow window that is wider than
   * the space and the right edge is clipped; in a wide one it is a small sheet in the middle of nothing.
   * The fit is what makes the window's size mean something, and it runs once: a reader who zooms has said
   * what they want, and a fitting pass that kept overriding them would be a fight.
   */
  let manuallyZoomed = false;
  $effect(() => {
    // Re-runs when the window changes size or the window's mode changes: the fit has to follow the mode
    // (two, three, four per row, full screen), and a size that only applied to the first layout would be a
    // fit for a window that no longer exists.
    if (manuallyZoomed || sizes === null || viewportWidth <= 0 || viewportHeight <= 0) return;
    const widest = sizes.reduce((width, size) => Math.max(width, size?.width ?? 0), 0);
    const first = sizes[initialPage] ?? sizes[0];
    if (widest <= 0 || first === undefined || first === null) return;
    const byWidth = viewportWidth / widest;
    const byHeight = viewportHeight / first.height;
    // Never tiny, never absurd: the smaller of the two axes, inside the zoom the reader can reach.
    scale = clampScale(Math.min(byWidth, Math.max(byHeight, 0.6)));
  });

  /**
   * Brings `jumpToPage` to the top of the viewport when the screen asks for it.
   *
   * `lastJump` remembers the request already served, so a re-render does not scroll again and fight the
   * reader's own scrolling. Asking twice for the same page means clearing the request in between.
   */
  let lastJump: number | null = null;
  $effect(() => {
    const target = jumpToPage;
    if (target === null || target === undefined) return;
    if (layout === null || target === lastJump) return;
    const box = layout.boxes[target];
    if (box === undefined) return;
    lastJump = target;
    void tick().then(() => {
      if (!scroller) return;
      scroller.scrollTop = box.top + stackOrigin();
      scrollTop = scroller.scrollTop - stackOrigin();
    });
  });

  const estimateNote = $derived.by(() => {
    if (sizes === null || measured === 0 || measured >= sizes.length) return null;
    return `Pages after ${measured} are laid out at page ${measured}'s size until they are acquired.`;
  });

  function reasonOf(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  /**
   * Where the stack starts inside the scroller's scrollable content.
   *
   * Read from the elements rather than computed from the note's text: the note wraps, changes when the
   * last page is measured, and its height is the browser's answer, not a constant this file could guess.
   */
  function stackOrigin(): number {
    return stack?.offsetTop ?? 0;
  }

  function canvasKey(atGeneration: number, pageNumber: number): string {
    return `${atGeneration}:${pageNumber}`;
  }

  function pageKey(pageNumber: number): string {
    return canvasKey(sessionGeneration, pageNumber);
  }

  /**
   * Creates the owner for one document.
   *
   * Every closure the session calls is bound to it: the canvas lookup uses its generation, the change
   * report only writes state if it is still the current session, and a measured size only corrects the
   * layout if it is still the current session. Nothing an old session does can reach the new one.
   */
  function createSession(loaded: ReaderDocument): PageSession {
    // Definite assignment: the callbacks below can only run after the constructor has returned — nothing
    // in an idle session reports a change or a size — so they always see the constructed session.
    let created!: PageSession;
    const canvasFor = (pageNumber: number): HTMLCanvasElement | undefined =>
      canvases.get(canvasKey(created.generation, pageNumber));

    created = new PageSession({
      document: loaded,
      generation: (generation += 1),
      pixelRatio,
      canvas: canvasFor,
      changed: () => {
        if (live && session === created) snapshot = created.renders.snapshot();
      },
      measured: (pageNumber, size) => {
        if (session === created) correctPageSize(pageNumber, size);
      },
    });
    return created;
  }

  /**
   * Applies a layout-derived scroll offset once the DOM has the stack's new height.
   *
   * The offset is also written to the reader's own state immediately, because that state is the position
   * the next zoom or correction anchors on; only the DOM write waits for the stack to claim its height.
   * A newer restore, an intervening scroll or a replaced document drops the pending write instead of
   * undoing the reader's position or an older zoom.
   */
  function scheduleRestore(offset: number | null): void {
    if (offset === null) return;
    const token = restores.begin();
    const owner = session;
    scrollTop = offset;
    void tick().then(() => {
      if (!live || owner === null || session !== owner || !restores.isCurrent(token)) return;
      if (scroller === undefined) return;
      scroller.scrollTop = offset + stackOrigin();
      scrollTop = scroller.scrollTop - stackOrigin();
    });
  }

  /**
   * Replaces a page's estimated size with the one it really has, keeping the reader where they were.
   *
   * This is what stops a differently shaped page — a landscape page after five hundred portraits — from
   * being drawn into a portrait placeholder: the box is corrected, and the anchor (page + the fraction of
   * it under the top of the viewport) is restored in the new layout.
   */
  function correctPageSize(pageNumber: number, size: PageSize): void {
    if (!live || sizes === null) return;
    const known = sizes[pageNumber - 1];
    if (
      known !== null &&
      known !== undefined &&
      known.width === size.width &&
      known.height === size.height
    ) {
      return;
    }

    const anchor: ScrollAnchor | null = layout === null ? null : anchorAt(layout, scrollTop);
    const next = withPageSize(sizes, pageNumber, size);
    sizes = next;
    if (anchor === null) return;

    scheduleRestore(
      offsetForAnchor(
        pageLayout(next, { scale, gap: PAGE_GAP, padding: PAGE_STACK_PADDING }),
        anchor,
      ),
    );
  }

  /**
   * Registers a slot's canvas for its session generation, and removes it when that slot leaves.
   *
   * The key is fixed when the action is created, so a slot destroyed by a document replacement removes its
   * own entry and never the new document's.
   */
  function canvasSlot(
    node: HTMLCanvasElement,
    params: { readonly generation: number; readonly page: number },
  ): { destroy(): void } {
    const key = canvasKey(params.generation, params.page);
    canvases.set(key, node);
    return {
      destroy() {
        if (canvases.get(key) === node) canvases.delete(key);
      },
    };
  }

  function onScroll(): void {
    if (scroller === undefined) return;
    // A reader's scroll is newer than any pending restore, so those restores are stale.
    restores.invalidate();
    scrollTop = scroller.scrollTop - stackOrigin();
    viewportHeight = scroller.clientHeight;
    viewportWidth = scroller.clientWidth;
    // The window is computed here rather than read from the derived one: the derived value updates after
    // this handler, and a report that waited for it would depend on the flush that has already failed once.
    if (layout !== null) {
      const window = pageWindow(layout, {
        scrollTop,
        viewportHeight,
        overscan,
        maxPages: maxWindowPages,
      });
      reportFirstVisible(window.visible);
    }
  }

  function failureFor(pageNumber: number): string | null {
    return snapshot.failed.find((failure) => failure.page === pageNumber)?.reason ?? null;
  }

  function isDrawn(pageNumber: number): boolean {
    return snapshot.drawn.includes(pageNumber);
  }

  /**
   * Whether a render is running for that page right now.
   *
   * A page whose render was cancelled is not drawing any more — it is queued behind the renders that are
   * still stopping — so the placeholder says it is waiting instead of claiming progress that is over.
   */
  function isDrawing(pageNumber: number): boolean {
    return snapshot.drawing.includes(pageNumber);
  }

  /**
   * Zooms the page view from outside, for the controls that live in the reader's bar.
   *
   * Exported rather than lifted: the scale belongs to this component because the layout is derived from
   * it, and a copy in the parent would be a second source of truth for the same number.
   */
  export function zoomBy(direction: "in" | "out"): void {
    zoom(direction);
  }

  function zoom(direction: "in" | "out"): void {
    // A reader who zooms has said what they want; the fit stops second-guessing them until a reload.
    manuallyZoomed = true;
    applyScale(zoomScale(scale, direction));
  }

  /**
   * Changes the zoom and keeps the reader where they were.
   *
   * Every drawn page becomes invalid at a new scale, so the stack's geometry changes under the scroll
   * offset and the offset alone would land somewhere else. The anchor — the page and the fraction of it
   * under the top of the viewport — is read before the change and applied after the stack has claimed
   * its new height; the layout is a pure function of the sizes and the zoom, so the target offset is
   * known before the DOM has it.
   */
  function applyScale(next: number): void {
    const clamped = clampScale(next);
    if (clamped === scale) return;

    const anchor: ScrollAnchor | null = layout === null ? null : anchorAt(layout, scrollTop);
    scale = clamped;
    if (anchor === null || sizes === null) return;

    scheduleRestore(
      offsetForAnchor(
        pageLayout(sizes, { scale: clamped, gap: PAGE_GAP, padding: PAGE_STACK_PADDING }),
        anchor,
      ),
    );
  }

  // One session per document. The sizes are read through it, with an abort that stops the pass where it
  // is; a different document retires the old session — no new acquisitions, its renders disposed, its
  // measured pages released — and starts from nothing.
  $effect(() => {
    const loaded = pdf;
    const current = createSession(loaded);
    const controller = new AbortController();
    let currentPass = true;
    let completed = false;

    untrack(() => {
      sizes = null;
      measured = 0;
      loadFailure = null;
      restores.invalidate();
      snapshot = current.renders.snapshot();
      session = current;
    });

    void readPageSizes((page) => current.measurePageSize(page), loaded.numPages, {
      signal: controller.signal,
    })
      .then((progress) => {
        completed = true;
        if (!currentPass || session !== current) return;
        sizes = progress.sizes;
        measured = progress.measured;
      })
      .catch((error: unknown) => {
        if (!currentPass || session !== current || error instanceof MeasurementStopped) return;
        loadFailure = reasonOf(error);
      })
      .finally(() => {
        // A pass that did not finish leaves pages it acquired that the layout will never use; a pass that
        // finished keeps them, because the next render may use the same proxies.
        if (!completed) current.releaseMeasuredPages();
      });

    return () => {
      currentPass = false;
      controller.abort();
      // The old document gives back what it measured and is retired, so it can never acquire another page
      // or start another render. Work it already had in flight releases its OWN canvas and page when it
      // settles, through the handles that captured them.
      current.releaseMeasuredPages();
      current.retire();
      restores.invalidate();
      if (session === current) session = null;
    };
  });

  // The window is the view: whenever the scroll, the zoom or the sizes say the visible pages changed, the
  // current session's render set is told. It is an effect so the canvases the new window needs already
  // exist when the set asks to draw them.
  //
  // …but they do not always exist yet, which is why the same view is applied a second time after the DOM
  // has caught up. A resume writes the scroll offset from inside an effect, so the slots for the page it
  // lands on are created one flush AFTER the window that names them, and the first application of that
  // window finds no canvas for any of its pages — a resumed document that drew nothing at all. Applying
  // the same view again once the slots exist draws them: the set retries the pages whose only failure was
  // a missing canvas, and a page that failed for its own sake is not drawn twice.
  $effect(() => {
    const current = session;
    if (current === null || layout === null || plan === null) return;
    current.renders.setView(plan.pages, scale);
    untrack(() => {
      void tick().then(() => {
        if (!live || session !== current || plan === null) return;
        current.renders.setView(plan.pages, scale);
      });
    });
  });

  onMount(() => {
    const measure = (): void => {
      if (scroller === undefined) return;
      viewportHeight = scroller.clientHeight;
    viewportWidth = scroller.clientWidth;
      scrollTop = scroller.scrollTop - stackOrigin();
    };
    measure();

    // A window resize or a layout change moves the viewport without a scroll; a browser without
    // ResizeObserver simply relies on the scroll listener rather than failing to mount the reader.
    const observer =
      typeof ResizeObserver === "function" && scroller !== undefined
        ? new ResizeObserver(measure)
        : null;
    if (observer !== null && scroller !== undefined) observer.observe(scroller);

    return () => {
      live = false;
      restores.invalidate();
      observer?.disconnect();
      // The document effect retires its session too; `retire` is idempotent. Turning `live` off first
      // stops any late settle from reporting into a component that no longer exists.
      session?.retire();
    };
  });
</script>

<!--
  The reader takes the whole window it is given (S3), so the stack scrolls inside this section rather
  than the page around it. The toolbar is the only chrome: zoom, where the reader is, and the count.
  Every page states its own condition — drawing, failed with a retry — instead of leaving an empty white
  rectangle that looks like a blank page of the document.
-->
<section class="reader" aria-label="Document pages">

  <!-- The page number, announced rather than drawn: the margin is decoration for a sighted reader, and
       hiding this element visually must not silence it. It lived inside the toolbar until that row was
       folded into the reader's one bar. -->
  <p class="position" aria-live="polite">{position}</p>

  <!-- The scroller is a region a keyboard can enter: without a focus stop, a keyboard user cannot scroll
       the pages at all, because the scroller is the element with the overflow. Svelte's rule cannot see
       that the tabindex exists for scrolling rather than for a click handler. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="scroller"
    bind:this={scroller}
    role="region"
    aria-label="Pages"
    aria-busy={busy}
    tabindex="0"
    onscroll={onScroll}
  >
    {#if loadFailure !== null}
      <p class="load-failure" role="alert">This document cannot be shown: {loadFailure}</p>
    {:else if layout === null || plan === null || session === null}
      <p class="measuring" role="status">Measuring the document's pages…</p>
    {:else}
      {#if estimateNote !== null}
        <p class="estimate" role="note">{estimateNote}</p>
      {/if}
      <div
        class="stack"
        bind:this={stack}
        style:height={`${layout.height}px`}
        style:width={`${layout.width}px`}
      >
        {#each boxes as box (pageKey(box.page))}
          <div
            class="slot"
            data-page={box.page}
            style:top={`${box.top}px`}
            style:left={`${box.left}px`}
            style:width={`${box.width}px`}
            style:height={`${box.height}px`}
          >
            <!-- A drawn page IS an image, and its number is its only accessible name: `role="img"` is the
                 correct role for it. Svelte's heuristic flags a canvas because a canvas may hold
                 interactive fallback content, which this one never does. -->
            <!-- svelte-ignore a11y_no_interactive_element_to_noninteractive_role -->
            <canvas
              use:canvasSlot={{ generation: sessionGeneration, page: box.page }}
              role="img"
              aria-label={`Page ${box.page}`}
            ></canvas>
            {#if failureFor(box.page) !== null}
              <div class="page-failure" role="alert">
                <p>Page {box.page} could not be drawn: {failureFor(box.page)}</p>
                <button
                  type="button"
                  class="retry"
                  aria-label={`Retry page ${box.page}`}
                  onclick={() => session?.retry(box.page)}>Try again</button
                >
              </div>
            {:else if !isDrawn(box.page)}
              <div class="page-loading" role="status">
                <span class="pulse" aria-hidden="true"></span>
                <span
                  >{isDrawing(box.page)
                    ? `Drawing page ${box.page}…`
                    : `Waiting to draw page ${box.page}…`}</span
                >
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  /* The reader is one pane inside whatever window it is given: it fills the height it is handed and
     scrolls the stack inside itself. `min-height: 0` is what lets a flex child shrink instead of pushing
     the toolbar out of the window. */
  /* The frame belongs to the screen that owns the window, so the bar, the pages and the footer sit inside
     ONE surface. This section only stacks the zoom row and the scroller. */
  .reader {
    display: flex;
    flex-direction: column;
    /* It fills the flex cell it is given instead of resolving a percentage against it: a percentage here
       collapses to `auto` whenever the parent has no definite height, and the scroller inside then grows
       to the whole document and never scrolls. */
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
  }







  /* The page number is drawn in the margin by the screen that owns the frame. This element stays for
     assistive technology: it is the live region that announces a page change, and hiding it visually
     must not silence it. */
  .position {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }


  .position {
    margin: 0;
  }

  .scroller {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    /* The stack's `offsetTop` is measured against this element, which is how the reader knows how much
       content — the estimation note — sits above the stack. */
    position: relative;
    /* No vertical padding: the stack's own padding is part of the layout, so `scrollTop` and the page
       coordinates share one origin. Padding here would shift every window and zoom-anchor decision by
       that amount, enough to miss a page entering the last pixels of the viewport. */
    padding: 0;
    /* No background of its own: the paper floats on the application's canvas. The gradient that used to
       be here painted a lighter panel around the page, which read as a frame wider than the paper and as
       the reason a wide window looked like a page inside a box inside a window. */
    /* The rail in the margin is the position indicator; a scrollbar inside the paper area would be a
       second one, and it eats width the page needs. */
    scrollbar-width: none;
  }

  .scroller::-webkit-scrollbar {
    width: 0;
    height: 0;
  }

  .scroller:focus-visible {
    outline-offset: -2px;
  }

  .measuring,
  .estimate {
    margin: 0 auto var(--space-4);
    max-width: 32rem;
    padding: 0 var(--space-4);
    font-size: var(--text-sm);
    line-height: 1.5;
    color: var(--fg-muted);
    text-align: center;
  }

  .estimate {
    color: var(--fg-subtle);
  }

  .load-failure {
    max-width: 32rem;
    margin: var(--space-4) auto;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--surface-1);
    color: var(--danger);
    font-size: var(--text-sm);
    line-height: 1.5;
    text-align: center;
  }

  /* The stack is the scroll height, and the slots are absolutely placed inside it: only the pages in the
     window exist in the DOM, so a thousand-page document is still a few nodes. */
  .stack {
    position: relative;
    margin: 0 auto;
  }

  /* The page is paper, not a card: it is the one surface in the application that is not the theme's
     dark glass, because a PDF is a printed page and pretending otherwise would tint every drawing. */
  .slot {
    position: absolute;
    overflow: hidden;
    border-radius: 2px;
    background: #fdfcf9;
    box-shadow: 0 18px 44px rgba(4, 2, 10, 0.48), 0 0 0 1px rgba(255, 255, 255, 0.05);
  }

  .slot canvas {
    display: block;
    width: 100%;
    height: 100%;
  }

  /* While a page is being drawn the paper would otherwise read as a blank page. The scrim is the dark
     theme laid over the paper so the message keeps the contrast the rest of the interface has. */
  .page-loading,
  .page-failure {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-4);
    text-align: center;
    font-size: var(--text-sm);
    color: var(--fg);
  }

  .page-loading {
    background: rgba(10, 7, 20, 0.55);
    backdrop-filter: blur(1.5px);
    -webkit-backdrop-filter: blur(1.5px);
  }

  .page-failure {
    background: rgba(10, 7, 20, 0.9);
    gap: var(--space-3);
  }

  .page-failure p {
    margin: 0;
    max-width: 24rem;
    line-height: 1.5;
  }

  .pulse {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: var(--r-full);
    background: var(--brand);
    box-shadow: 0 0 0 0 var(--brand-glow);
    animation: breathe 1.4s var(--ease) infinite;
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 0.35;
      transform: scale(0.8);
    }
    50% {
      opacity: 1;
      transform: scale(1);
    }
  }

  /* The one loud control in the palette is a white fill with dark text; a failed page is the one place
     the reader has to act, so it gets it. */
  .retry {
    padding: var(--space-1) var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: var(--fg);
    color: var(--fg-on-loud);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    cursor: pointer;
  }

  .retry:hover {
    background: #fff;
  }
</style>
