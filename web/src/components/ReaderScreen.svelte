<script lang="ts">
  // The reader screen: it opens one document and hands it to the continuous page view.
  //
  // The division of labour is deliberate. `ReaderPages` owns the reading surface and never opens
  // anything; `../lib/reader-source` owns the bytes and never draws. This file is the small piece
  // between them: it holds the loading state, the failure state, and the way out.
  //
  // It takes the window (S3). The window's own bar above it stays, because the window is undecorated
  // and that bar is the only way to move or close it; what goes away is the navigation and the screens.
  //
  // The chrome is the margin (design/pdf-reader/03-margins.html): the page number and the progress rail
  // live in a gutter beside the paper, so nothing ever covers the document — the one thing a reader is
  // looking at. The controls sit in the top bar and the percentage in the bottom one, both outside the
  // page's own width.
  import { onMount } from "svelte";
  import ReaderPages from "./ReaderPages.svelte";
  import { openReaderDocument, type ReaderSource } from "../lib/reader-source";
  import { invoke } from "../lib/tauri";
  import { createPageRecorder } from "../lib/reader-progress";
  import { createReadingSession } from "../lib/reader-session";
  import type { LibraryEntry } from "../lib/library";
  import type { Translate } from "../lib/i18n";

  let {
    entry,
    t,
    onClose,
    onFallback,
  }: {
    entry: LibraryEntry;
    /** The interface's word catalogue. */
    t: Translate;
    /** Leaves the reader, back to the screen it was opened from. */
    onClose: () => void;
    /** Opens the document with the system's own application, when the reader cannot lay it out. */
    onFallback: () => void;
  } = $props();

  /** The loaded document, or `null` while it is still being opened. */
  let source = $state<ReaderSource | null>(null);
  /** Why the document could not be opened at all, if it could not. */
  let failure = $state<string | null>(null);
  /** The page the store remembers for this document, zero-based; `null` when it has never heard of one. */
  let position = $state<number | null>(null);

  /** The first page on screen, zero-based, bound from the page view. The gutter's number is this plus one. */
  let currentPage = $state(0);
  /** The document's own cover, as a URL the webview can load, or `null` while it is unknown. */
  let cover = $state<string | null>(null);
  /** Whether the footer is asking for a page number. */
  let asking = $state(false);
  /** What the reader typed in the footer. */
  let typed = $state("");
  /** The page the reader asked to jump to, zero-based; the page view scrolls to it and it is cleared. */
  let jumpToPage = $state<number | null>(null);
  /** The document's page count, when the library knows it. */
  const total = $derived(entry.pages ?? null);
  /**
   * How far through the document the reader is, as a whole number.
   *
   * `null` when the page count is unknown: a rail with no total would be a rail invented from nothing,
   * and a percentage nobody can compute is worse than no percentage.
   */
  const percent = $derived(
    total === null ? null : Math.min(100, Math.max(0, Math.round(((currentPage + 1) / total) * 100))),
  );

  /** Turns page turns into one stored fact, and holds the last one until the reader closes (S4). */
  let recorder: ReturnType<typeof createPageRecorder> | null = null;

  /**
   * The reading session: what the window says about being on screen, and what the reader does with it.
   *
   * It starts when the document actually opens — a document that failed to open is not reading time —
   * and its facts are written once, on the way out, because a page visit names the session it happened
   * in and the session therefore has to exist first (R3b).
   */
  let session: ReturnType<typeof createReadingSession> | null = null;

  /**
   * Sends the typed page to the page view.
   *
   * The number the reader types is one-based, like every page number they see; the view speaks zero-based
   * offsets, so the conversion happens here and nowhere else. A number outside the document is clamped to
   * its own ends rather than refused: the reader meant a page, and the nearest real one is the answer.
   */
  function jump(): void {
    // `bind:value` on a number input hands back a NUMBER, so the field is stringified before it is parsed:
    // calling `.trim()` on it threw inside the handler and the button did nothing at all.
    const asked = Number.parseInt(String(typed).trim(), 10);
    asking = false;
    typed = "";
    if (!Number.isFinite(asked)) return;
    const last = total === null ? asked : total;
    const page = Math.min(Math.max(asked, 1), Math.max(last, 1));
    jumpToPage = page - 1;
  }

  onMount(() => {
    // A screen that is replaced before the document settles must not keep it: the loader owns a worker
    // and a transport, and neither has an owner left once this component is gone.
    let live = true;

    /**
     * Recomputes whether time should be counted at all.
     *
     * Both halves of L4's rule are read from the window itself rather than assumed: a hidden window is
     * not being read, and neither is an unfocused one, even while its pages are still on screen.
     */
    function syncActivity(): void {
      session?.setActive(
        document.visibilityState === "visible" && document.hasFocus(),
      );
    }

    // Any of these is the reader doing something, which is what lifts an idle pause.
    const onInteraction = (): void => {
      session?.interact();
    };
    const onFocus = (): void => {
      session?.interact();
      syncActivity();
    };

    document.addEventListener("visibilitychange", syncActivity);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", syncActivity);
    window.addEventListener("wheel", onInteraction, { passive: true });
    window.addEventListener("keydown", onInteraction);
    window.addEventListener("pointerdown", onInteraction);

    // The page and the document are asked for at the same time: the reader must not open at page one and
    // then jump, and the store's answer is needed before the page view exists.
    const remembered = invoke<number | null>("reader_position", { id: entry.id }).catch(() => null);
    // The cover names the book beside its title; without one the mark stands in, which is what a document
    // whose cover could not be rendered looks like everywhere else in the application.
    void invoke<string | null>("document_cover", { id: entry.id })
      .then((source) => {
        if (live) cover = source;
      })
      .catch(() => {});

    void Promise.all([openReaderDocument(entry.path), remembered]).then(
      ([opened, page]) => {
        if (!live) {
          opened.destroy();
          return;
        }
        position = page;
        currentPage = page ?? 0;
        recorder = createPageRecorder((changed) =>
          invoke("reader_remember_page", { id: entry.id, page: changed }).then(() => undefined),
        );
        session = createReadingSession({ now: () => Date.now() });
        // The page the reader opened at is on screen before any scroll can report it.
        session.page(position ?? 0);
        syncActivity();
        source = opened;
      },
      (error: unknown) => {
        if (live) failure = String(error);
      },
    );

    return () => {
      live = false;
      document.removeEventListener("visibilitychange", syncActivity);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", syncActivity);
      window.removeEventListener("wheel", onInteraction);
      window.removeEventListener("keydown", onInteraction);
      window.removeEventListener("pointerdown", onInteraction);
      // The last page of a session is the one a resume needs, so it is written before the recorder goes
      // away rather than lost with the quiet period it was waiting out.
      void recorder?.flush();
      recorder?.stop();
      recorder = null;
      // The session is written once, at the end, because its visits name it. A write that fails is
      // swallowed for the same reason a page write is: a reader closing a book must not be interrupted
      // by bookkeeping, and there is no retry queue to feed.
      const facts = session?.finish();
      session = null;
      if (facts) {
        // Called exactly as `reader_remember_page` is: outside Tauri the call rejects and is swallowed,
        // which is the same stance as a page write that could not be stored.
        void invoke("reader_record_session", { id: entry.id, facts }).catch(() => {});
      }
      source?.destroy();
      source = null;
    };
  });
</script>

<section class="screen" aria-label={entry.title}>
  <header class="top">
    {#if cover !== null}
      <img class="cover" src={cover} alt="" aria-hidden="true" />
    {:else}
      <span class="mark" aria-hidden="true"></span>
    {/if}
    <p class="title">{entry.title}</p>
    <button class="close" type="button" onclick={onClose}>{t("reader.close")}</button>
  </header>

  {#if failure !== null}
    <div class="state">
      <p>{t("reader.failed", { message: failure })}</p>
      <button type="button" onclick={onFallback}>{t("reader.fallback")}</button>
    </div>
  {:else if source === null}
    <p class="state">{t("reader.opening")}</p>
  {:else}
    <div class="body">
      <!-- The margin does the bookkeeping: where you are, the rail, and how long the book is. It is
           decoration for a sighted reader — `ReaderPages` keeps the live region that announces the same
           page to assistive technology — so it is hidden from the accessibility tree on purpose. -->
      <aside class="gutter" aria-hidden="true">
        <span class="page">{currentPage + 1}</span>
        <span class="rail"><i style:height={`${percent ?? 0}%`}></i></span>
        <span class="total">{total ?? "—"}</span>
      </aside>
      <div class="stage">
        <ReaderPages
          document={source.document}
          initialPage={position ?? 0}
          bind:currentPage
          {jumpToPage}
          onPageChange={(page) => {
            recorder?.observe(page);
            session?.page(page);
            session?.interact();
          }}
        />
      </div>
    </div>

    <footer class="foot">
      {#if asking}
        <form class="ask" onsubmit={(event) => { event.preventDefault(); jump(); }}>
          <label class="ask-label" for="reader-jump">Ir a la página</label>
          <input
            id="reader-jump"
            class="ask-input"
            type="number"
            min="1"
            max={total ?? undefined}
            bind:value={typed}
            onblur={() => { asking = false; typed = ""; }}
          />
        </form>
      {:else}
        <button class="jump" type="button" onclick={() => (asking = true)}>Ir a la página…</button>
      {/if}
      {#if percent !== null}
        <span class="percent">{percent} %</span>
      {/if}
    </footer>
  {/if}
</section>

<style>
  /* A column that fills whatever the shell gives it: the bars are fixed height, the pages take the rest.
     `min-height: 0` is what lets the pages shrink instead of pushing the bar out of the window. */
  /* Bar, pages and footer live inside ONE surface: the reader is a sheet of paper in a frame, not a panel
     inside another panel. */
  .screen {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
  }

  .top {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-3, 12px);
    padding: 10px 14px;
  }

  /* The mark, not a logo: it stands in for a cover that could not be rendered. */
  .mark {
    flex: none;
    width: 20px;
    height: 26px;
    border-radius: var(--r-xs, 4px);
    background: var(--brand);
  }

  /* The book's own cover, at the size of a spine label: enough to recognise the title next to it. */
  .cover {
    flex: none;
    width: 20px;
    height: 26px;
    object-fit: cover;
    border-radius: var(--r-xs, 4px);
    border: 1px solid var(--line);
    background: var(--surface-1);
  }

  .title {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The one loud control on this screen: leaving is the only thing here that is not reading. */
  .close {
    flex: none;
    height: 28px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-sm, 6px);
    background: #fff;
    color: var(--fg-on-loud, #0b0714);
    font: inherit;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    cursor: pointer;
  }

  .close:hover {
    background: var(--fg);
  }

  .close:focus-visible {
    outline: 2px solid var(--line-focus);
    outline-offset: 2px;
  }

  /* `minmax(0, 1fr)` on BOTH axes is what makes the page view scrollable: with an `auto` row the stack
     grows to its full height, the scroller never overflows, and nothing on the screen can move — which
     looks exactly like a reader whose progress tracker is broken. */
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 56px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
  }

  /* The gutter: page number, rail, total — read top to bottom, so the rail fills the way a book runs. */
  .gutter {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4, 16px);
  }

  .page,
  .total {
    writing-mode: vertical-rl;
    color: var(--fg-subtle);
    font-variant-numeric: tabular-nums;
  }

  .page {
    font-family: var(--font-display, Georgia, serif);
    font-size: var(--text-base);
    letter-spacing: 0.16em;
  }

  .total {
    font-size: var(--text-xs);
    letter-spacing: var(--tracking-wide, 0.14em);
  }

  /* The only saturated thing on the screen, because progress is the one fact the reader is here for. */
  .rail {
    position: relative;
    flex: 0 0 180px;
    width: 4px;
    border-radius: var(--r-full, 999px);
    background: rgba(255, 255, 255, 0.09);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
    overflow: hidden;
  }

  .rail i {
    position: absolute;
    inset: 0 0 auto 0;
    border-radius: var(--r-full, 999px);
    background: linear-gradient(to bottom, var(--brand), var(--brand-mid));
    /* The bright edge is where the reader is: the one mark that has to be findable at a glance. */
    box-shadow: inset 0 -2px 0 rgba(255, 255, 255, 0.92);
  }

  .stage {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding-right: 8px;
  }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    height: 32px;
    padding: 0 14px;
    border-top: 1px solid var(--line);
    font-size: var(--text-xs);
    color: var(--fg-subtle);
  }

  .percent {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }

  /* The one action the footer carries: jumping is the only way to move without scrolling. */
  .jump {
    padding: 3px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-sm, 6px);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .jump:hover {
    border-color: var(--line-strong);
    color: var(--fg);
  }

  .ask {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .ask-label {
    color: var(--fg-subtle);
  }

  .ask-input {
    width: 56px;
    padding: 2px 6px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-xs, 4px);
    background: var(--surface-1);
    color: var(--fg);
    font: inherit;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }

  /* At the narrowest window the margin costs too much of the page, so it gives way first. */
  @media (max-width: 520px) {
    .body {
      grid-template-columns: 40px minmax(0, 1fr);
    }

    .rail {
      flex-basis: 132px;
    }
  }

  .state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 24px;
    color: var(--fg-muted);
    font-size: var(--text-sm);
    text-align: center;
  }

  .state button {
    padding: 6px 14px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm, 8px);
    background: transparent;
    color: var(--fg);
    font: inherit;
    cursor: pointer;
  }
</style>
