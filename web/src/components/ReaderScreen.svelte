<script lang="ts">
  // The reader screen: it opens one document and hands it to the continuous page view.
  //
  // The division of labour is deliberate. `ReaderPages` owns the reading surface and never opens
  // anything; `../lib/reader-source` owns the bytes and never draws. This file is the small piece
  // between them: it holds the loading state, the failure state, and the way out.
  //
  // It takes the window (S3). The window's own bar above it stays, because the window is undecorated
  // and that bar is the only way to move or close it; what goes away is the navigation and the screens.
  import { onMount } from "svelte";
  import ReaderPages from "./ReaderPages.svelte";
  import { openReaderDocument, type ReaderSource } from "../lib/reader-source";
  import { invoke, runningInTauri } from "../lib/tauri";
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

  onMount(() => {
    // A screen that is replaced before the document settles must not keep it: the loader owns a worker
    // and a transport, and neither has an owner left once this component is gone.
    let live = true;
    const started = performance.now();

    void openReaderDocument(entry.path).then(
      (opened) => {
        if (!live) {
          opened.destroy();
          return;
        }
        // The number 0.2.0 turns on: how long a real document takes to become readable, and what the
        // ranges cost. The webview's console is not visible from the terminal that started the
        // application, so the line goes through the desktop process; a browser without it, or a
        // command that is not registered, is not a failure the reader should ever surface.
        const ms = Math.round(performance.now() - started);
        console.info(`reader opened in ${ms} ms`, { path: entry.path, ...opened.stats });
        if (runningInTauri()) {
          void invoke("reader_measured", {
            line: JSON.stringify({ path: entry.path, ms, ...opened.stats }),
          }).catch(() => {});
        }
        source = opened;
      },
      (error: unknown) => {
        if (live) failure = String(error);
      },
    );

    return () => {
      live = false;
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
    <div class="pages">
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
