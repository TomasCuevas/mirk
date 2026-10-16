<script lang="ts">
  import { onMount } from "svelte";
  import type { Translate } from "../lib/i18n";
  import {
    categoryEntries,
    currentUnixSeconds,
    homeCategories,
    startCategoryRefresh,
    type FolderActionStatus,
    type HomeCategory,
    type LibraryEntry,
  } from "../lib/library";
  import { categoryDefinition, categoryTitle } from "../lib/screens";
  import BookGrid from "./BookGrid.svelte";

  let {
    entries,
    loading,
    error,
    t,
    folderStatus,
    onOpenFolder,
    onOpenCategory,
    onNavigate,
  }: {
    entries: LibraryEntry[];
    loading: boolean;
    error: string | null;
    t: Translate;
    folderStatus: FolderActionStatus | null;
    onOpenFolder: (entry: LibraryEntry) => void;
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

  {#if loading}
    <p class="state">{t("library.loading")}</p>
  {:else if error !== null}
    <p class="state error">{t("library.error", { message: error })}</p>
  {:else}
    {#if nothingToShow}
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
          </button>
        </div>

        <!-- B4: the definition is on screen because the category is a reading of the stored position,
             not a fact the document carries. The stale definition carries its own threshold. -->
        <p class="definition">{categoryDefinition(section.category, t)}</p>

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
          <BookGrid entries={section.items} {t} {folderStatus} {onOpenFolder} layout="strip" />
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
    background: #f7f9fb;
    border: 1px solid #e2e6eb;
    border-radius: 0.6rem;
  }

  .first-run h2 {
    margin: 0;
    font-size: 1rem;
    color: #17364a;
  }

  .first-run p {
    margin: 0;
    font-size: 0.9rem;
    line-height: 1.45;
    color: #5a6472;
  }

  .first-run button {
    font: inherit;
    font-size: 0.9rem;
    padding: 0.4rem 0.8rem;
    color: #24506b;
    background: #ffffff;
    border: 1px solid #ccd2da;
    border-radius: 0.5rem;
    cursor: pointer;
  }

  h1 {
    margin: 0;
    font-size: 1.35rem;
  }

  .carousel {
    /* The tile width is declared inline on the element so it is a single, inspectable value; BookGrid's
       strip and the empty slot below both read it as `--tile`. */
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.6rem;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  .head button,
  .definition,
  .state {
    margin: 0;
  }

  .head button {
    flex: none;
    padding: 0.35rem 0.55rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .head button:hover {
    background: #f0f2f5;
  }

  .definition {
    font-size: 0.8rem;
    color: #5a6472;
  }

  .state {
    color: #5a6472;
  }

  /* The empty slot is the same rectangle as a real card: the same width, padding and border, a cover
     with the same 9/11 ratio, and the same fixed-height text block below it. They are the same size
     because they are built from the same numbers, not because two sets of numbers were tuned to agree —
     which is what the user asked for. */
  /* The same chrome the real card has — white, solid border, same radius — so the empty slot reads as
     a card-shaped slot rather than as a dashed box drawn around the cover placeholder. The first
     attempt copied the card's measurements but kept a dashed translucent frame, and the result was a
     frame inside a frame (L31). */
  .empty {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    width: var(--tile);
    padding: 0.7rem;
    box-sizing: border-box;
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
  }

  .empty-cover {
    /* Same `border-box` ratio as the card's cover, so both are the same rectangle. */
    box-sizing: border-box;
    border: 1px dashed #ccd2da;
    border-radius: 0.4rem;
    background: repeating-linear-gradient(45deg, #f6f7f9, #f6f7f9 8px, #eef0f3 8px, #eef0f3 16px);
  }

  .empty-body {
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    color: #8b95a3;
    font-size: 0.8rem;
  }

  .empty-body p {
    margin: 0;
  }

  .state.error {
    color: #a1301f;
  }
</style>
