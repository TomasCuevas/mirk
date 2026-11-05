<script lang="ts">
  // The reader screen: it opens one document and hands it to the continuous page view.
  //
  // The division of labour is deliberate. `ReaderPages` owns the reading surface and never opens
  // anything; `../lib/reader-source` owns the bytes and never draws. This file is the small piece
  // between them: it holds the loading state, the failure state, and the way out.
  //
  // It takes the window (S3). The window's own bar above it stays, because the window is undecorated
  // and that bar is the only way to move or close it; what goes away is the navigation and the screens.
  import { onMount, tick } from "svelte";
  import ReaderPages from "./ReaderPages.svelte";
  import { openReaderDocument, type ReaderSource } from "../lib/reader-source";
  import {
    samplePaint,
    probeFirstPage,
    probeRenderOnly,
    probeFrames,
    collectErrors,
  } from "../lib/reader-measure";
  import { invoke, runningInTauri } from "../lib/tauri";
  import type { LibraryEntry } from "../lib/library";
  import type { Translate } from "../lib/i18n";

  let {
    entry,
    t,
    onClose,
    onFallback,
    measure = false,
  }: {
    entry: LibraryEntry;
    /** The interface's word catalogue. */
    t: Translate;
    /** Leaves the reader, back to the screen it was opened from. */
    onClose: () => void;
    /** Opens the document with the system's own application, when the reader cannot lay it out. */
    onFallback: () => void;
    /**
     * Whether this open is a measurement run, which also reports what ended up on the canvases.
     *
     * Only `MIRK_READER_MEASURE` sets it, so a normal open reports the load and samples nothing.
     */
    measure?: boolean;
  } = $props();

  /** The loaded document, or `null` while it is still being opened. */
  let source = $state<ReaderSource | null>(null);
  /** Why the document could not be opened at all, if it could not. */
  let failure = $state<string | null>(null);
  /** The surface the pages are drawn on, sampled only by a measurement run. */
  let pages: HTMLDivElement | undefined = $state();
  /** The webview's own errors, collected only by a measurement run: pdf.js reports failures that way. */
  let collected: { entries: string[]; stop: () => void } | null = null;

  /**
   * Sends the measurement of one load, and — in a measurement run — of what ended up on the canvases.
   *
   * The webview's console is not visible from the terminal that started the application, so the line goes
   * through the desktop process; a browser without it, or a command that is not registered, is not a
   * failure the reader should ever surface.
   */
  async function report(load: {
    ms: number;
    stats: ReaderSource["stats"];
    document: ReaderSource["document"];
  }): Promise<void> {
    console.info(`reader opened in ${load.ms} ms`, { path: entry.path, ...load.stats });
    const line: Record<string, unknown> = { path: entry.path, ms: load.ms, ...load.stats };

    if (measure) {
      // Two different questions, answered at once: does the page's content parse at all, and what ended up
      // on the canvas. A page stuck on `drawing` can be either, and the stage that stops answering says
      // which.
      await tick();
      const [frames, probe, render, paint] = await Promise.all([
        probeFrames().catch((error: unknown) => ({ error: String(error) })),
        probeFirstPage(load.document).catch((error: unknown) => ({ error: String(error) })),
        probeRenderOnly(load.document).catch((error: unknown) => ({ error: String(error) })),
        samplePaint(pages ?? document.body),
      ]);
      line.frames = frames;
      line.probe = probe;
      line.render = render;
      line.paint = paint;
    }

    if (collected !== null) line.errors = collected.entries;

    if (!runningInTauri()) return;
    void invoke("reader_measured", { line: JSON.stringify(line) }).catch(() => {});
  }

  onMount(() => {
    // A screen that is replaced before the document settles must not keep it: the loader owns a worker
    // and a transport, and neither has an owner left once this component is gone.
    let live = true;
    const started = performance.now();
    if (measure) collected = collectErrors();

    void openReaderDocument(entry.path).then(
      (opened) => {
        if (!live) {
          opened.destroy();
          return;
        }
        const ms = Math.round(performance.now() - started);
        source = opened;
        void report({ ms, stats: opened.stats, document: opened.document });
      },
      (error: unknown) => {
        if (live) failure = String(error);
      },
    );

    return () => {
      live = false;
      collected?.stop();
      collected = null;
      source?.destroy();
      source = null;
    };
  });
</script>

<section class="screen" aria-label={entry.title}>
  <header class="bar">
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
    <div class="pages" bind:this={pages}>
      <ReaderPages document={source.document} />
    </div>
  {/if}
</section>

<style>
  /* A column that fills whatever the shell gives it: the bar is fixed height, the pages take the rest.
     `min-height: 0` is what lets the pages shrink instead of pushing the bar out of the window. */
  .screen {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
  }

  .title {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .close {
    flex: none;
    padding: 4px 10px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm, 8px);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .close:hover {
    color: var(--fg);
    border-color: var(--line-focus);
  }

  .pages {
    flex: 1;
    min-height: 0;
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
