<script lang="ts">
  import { onMount } from "svelte";
  import type { Translate } from "../lib/i18n";
  import {
    categoryEntries,
    currentUnixSeconds,
    homeCategories,
    libraryAuthors,
    libraryView,
    startCategoryRefresh,
    type HomeCategory,
    type LibraryEntry,
  } from "../lib/library";
  import { categoryTitle } from "../lib/screens";
  import BookGrid from "./BookGrid.svelte";

  let {
    entries,
    inFlight,
    error,
    locale,
    t,
    onOpenFolder,
    onOpenDocument,
    onOpenCategory,
    onNavigate,
  }: {
    entries: LibraryEntry[];
    inFlight: boolean;
    error: string | null;
    locale: string;
    t: Translate;
    onOpenFolder: (entry: LibraryEntry) => void;
    /** Opens the document itself, in the system's own application for it (R37). */
    onOpenDocument: (entry: LibraryEntry) => void;
    onOpenCategory: (category: HomeCategory) => void;
    onNavigate: (target: "library" | "settings") => void;
  } = $props();

  // One instant for every carousel in a render, so "a long time" cannot differ between them (B3) —
  // and re-read on a tick, because the screen can stay open while a document crosses the staleness
  // threshold, which must move it between carousels without a remount.
  let now = $state(currentUnixSeconds());
  const sections = $derived(
    homeCategories.map((category) => ({
      category,
      items: categoryEntries(entries, category, now),
    })),
  );

  // R34: an Inicio with nothing to show has exactly TWO possible reasons, and the screen has to tell them
  // apart because they call for different actions — nothing has been found yet (the answer is a watched
  // folder), or documents exist and no source has recorded a reading (the answer is to open one, because
  // oct never opens a document itself: it observes reading done elsewhere). One vague sentence over three
  // empty carousels was exactly what left the user asking whether the screen worked at all.
  const nothingToShow = $derived(sections.every((section) => section.items.length === 0));

  // L47. What the screen shows is one decision, and it lives in `libraryView` so Inicio and
  // Biblioteca obey the same rule and a test can pin it without rendering anything. The loader is the
  // sentence the catalogue already had for the first load: "Loading the library…" is exactly as true
  // of a rescan or a folder removal as it is of the first read, so no second string was added. That
  // sentence IS the loader — a spinner would add motion without adding information.
  const view = $derived(libraryView(entries, inFlight));

  // R40/S1. The editor's author list comes from the WHOLE listing, not from the carousel slice each grid
  // is handed: an author whose only document a carousel does not show would otherwise not exist there.
  const authorSuggestions = $derived(libraryAuthors(entries, locale));

  // The tick is the shared helper, and it is stopped when the screen unmounts, so a screen that is
  // not open runs no timer.
  onMount(() => startCategoryRefresh(() => (now = currentUnixSeconds())));
</script>

<!--
  Inicio (R2, R4). The header is only what names the screen: the explanatory prose is gone. What
  replaces it is the summaries themselves — three carousels whose categories are stated on screen
  (B4), each scrollable sideways (B1) and each with the button that opens its full list (B2). A
  carousel with nothing in it says so instead of showing an empty strip (B1).
-->
<section class="screen">
  <h1>{t("home.heading")}</h1>

  {#if view === "loading"}
    <p class="state">{t("library.loading")}</p>
  {:else if error !== null}
    <p class="state error">{t("library.error", { message: error })}</p>
  {:else}
    {#if !inFlight && nothingToShow}
      <!-- R34. Only when there is nothing to show, so R2 still holds: the header names the screen, and a
           reader who has something in their library never sees this. It states the missing fact and offers
           the one action that can supply it — and it invents nothing, because an empty screen that fills
           itself with plausible-looking content is the lie this whole product exists to avoid. -->
      {@const emptyLibrary = entries.length === 0}
      <div class="first-run">
        <h2>{t(emptyLibrary ? "home.firstRun.emptyLibrary.title" : "home.firstRun.noRecords.title")}</h2>
        <p>
          {t(emptyLibrary ? "home.firstRun.emptyLibrary.body" : "home.firstRun.noRecords.body")}
        </p>
        <button type="button" onclick={() => onNavigate(emptyLibrary ? "settings" : "library")}>
          {t(emptyLibrary ? "home.firstRun.emptyLibrary.action" : "home.firstRun.noRecords.action")}
        </button>
      </div>
    {/if}
    {#each sections as section (section.category)}
      <section class="carousel" style="--tile: 13rem">
        <div class="head">
          <h2>{categoryTitle(section.category, t)}</h2>
          <!-- B2: the button is at the top of the carousel and opens the category's own screen. Its
               accessible name names the category, so three identical-looking buttons are three
               different destinations to a screen reader as well. -->
          <button
            type="button"
            aria-label={t("home.openCategoryLabel", {
              category: categoryTitle(section.category, t),
            })}
            onclick={() => onOpenCategory(section.category)}
          >
            {t("home.openCategory")}
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
              stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M3.5 8h9" />
              <path d="M8.5 4l4 4-4 4" />
            </svg>
          </button>
        </div>

        {#if section.items.length === 0}
          <!-- B1: an empty carousel keeps the place a card would take — the same cover placeholder, and
               the short message right under it — so the next heading is not pressed against this one.
               It is deliberately SHORTER than a carousel holding cards, because the user found a tile
               that reserved the whole text block too tall and too narrow: reserving that block was what
               made the card read as stretched, and the message floated in the middle of the emptiness
               it created (L31). -->
          <div class="empty">
            <div class="empty-cover" style="aspect-ratio: 9 / 11" aria-hidden="true"></div>
            <div class="empty-body">
              <p>{t("home.categoryEmpty")}</p>
            </div>
          </div>
        {:else}
          <BookGrid
            entries={section.items}
            authors={authorSuggestions}
            {t}
            {onOpenFolder}
            {onOpenDocument}
            layout="strip"
          />
        {/if}
      </section>
    {/each}
  {/if}
</section>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* The first-run note uses the same border and muted text as the rest of the application, so it reads as
     one more block of the screen rather than as an alert. */
  .first-run {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.85rem 1rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
  }

  .first-run h2 {
    margin: 0;
    font-size: var(--text-base);
    color: var(--fg);
  }

  .first-run p {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.45;
    color: var(--fg-muted);
  }

  .first-run button {
    font: inherit;
    font-size: var(--text-sm);
    padding: 0.4rem 0.8rem;
    color: var(--fg);
    background: var(--surface-1);
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    cursor: pointer;
    transition: background var(--fast) var(--ease), border-color var(--fast) var(--ease);
  }

  .first-run button:hover {
    background: var(--surface-2);
    border-color: var(--line-strong);
  }

  /* The screen's title is the one the token file defines: no component states its own size. */
  h1 {
    margin: 0;
  }

  .carousel {
    /* The tile width is declared inline on the element so it is a single, inspectable value; BookGrid's
       strip and the empty slot below both read it as `--tile`. */
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.6rem;
  }

  h2 {
    margin: 0;
  }

  .head h2 {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg);
  }

  .head button,
  .state {
    margin: 0;
  }

  /* The way out of a carousel is a word, not a chip: a button beside a heading would compete with the
     heading for the same glance, so it is drawn as text that says it can be clicked. */
  .head button {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: var(--text-xs);
    color: var(--fg-muted);
    cursor: pointer;
    transition: color var(--fast) var(--ease);
  }

  .head button:hover {
    color: var(--fg);
  }

  .state {
    color: var(--fg-muted);
  }

  /* The empty slot is the same rectangle as a real card: the same width, padding and border, a cover
     with the same 9/11 ratio, and the same fixed-height text block below it. They are the same size
     because they are built from the same numbers, not because two sets of numbers were tuned to agree —
     which is what the user asked for. */
  /* The same chrome the real card has — one surface, one hairline, same radius — so the empty slot reads
     as a card-shaped slot rather than as a dashed box drawn around the cover placeholder. The first
     attempt copied the card's measurements but kept a dashed translucent frame, and the result was a
     frame inside a frame (L31). */
  .empty {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    width: var(--tile);
    padding: 0.45rem;
    box-sizing: border-box;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
  }

  /* The same cover the card draws, rule for rule: the placeholder and a real cover are one object. */
  .empty-cover {
    /* Same `border-box` ratio as the card's cover, so both are the same rectangle. */
    box-sizing: border-box;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--surface-1);
    background-image: repeating-linear-gradient(
      45deg,
      transparent 0 9px,
      rgba(255, 255, 255, 0.018) 9px 18px
    );
    color: var(--fg-subtle);
    font-size: var(--text-xs);
  }

  .empty-body {
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
  }

  .empty-body p {
    margin: 0;
    color: var(--fg-subtle);
    font-size: var(--text-xs);
  }

  .state.error {
    color: var(--danger);
  }
</style>
