<script lang="ts">
  import { onMount } from "svelte";
  import type { Translate } from "../lib/i18n";
  import {
    categoryEntries,
    currentUnixSeconds,
    libraryAuthors,
    libraryView,
    startCategoryRefresh,
    type HomeCategory,
    type LibraryEntry,
  } from "../lib/library";
  import { categoryDefinition, categoryTitle } from "../lib/screens";
  import BookGrid from "./BookGrid.svelte";

  let {
    category,
    entries,
    inFlight,
    error,
    locale,
    t,
    onOpenFolder,
    onOpenDocument,
    onBack,
  }: {
    category: HomeCategory;
    entries: LibraryEntry[];
    inFlight: boolean;
    error: string | null;
    locale: string;
    t: Translate;
    onOpenFolder: (entry: LibraryEntry) => void;
    /** Opens the document itself, in the system's own application for it (R37). */
    onOpenDocument: (entry: LibraryEntry) => void;
    onBack: () => void;
  } = $props();

  // The same measurement Inicio uses, so the carousel and its full list cannot disagree about which
  // documents belong to the category (B3) — and re-read on the same tick, for the same reason: the
  // list can stay open while a record crosses the staleness threshold.
  let now = $state(currentUnixSeconds());
  const items = $derived(categoryEntries(entries, category, now));

  // L47. The screen decides through the same pure function as Inicio and Biblioteca, over the
  // category's OWN list: an empty category is only presented AS empty once nothing is in flight, and
  // a category that already shows documents never loses them to a loader. The loader is the catalogue's
  // existing loading sentence, which is as true of a scan as of the first read.
  const view = $derived(libraryView(items, inFlight));

  // R40/S1. The editor's author list is computed from the WHOLE listing, not from the category's own
  // items: the category is what this screen shows, and the authors a reader may want to type are every
  // author the library holds.
  const authorSuggestions = $derived(libraryAuthors(entries, locale));

  onMount(() => startCategoryRefresh(() => (now = currentUnixSeconds())));
</script>

<!--
  One category's full list (B2), a screen of its own rather than a filter bolted onto Biblioteca. It
  renders the same cards through the same component as the library, and it states the category's own
  definition at the top (B4), because "finished" and "in progress" are readings of a stored position
  rather than facts the document carries.
-->
<section class="screen">
  <!-- A way back that does not depend on the bottom navigation. It calls the same navigation the
       navbar's Inicio button uses (App wires it to `selectScreen("home")`), so returning from a
       category uses the existing routing rather than a second mechanism. -->
  <button type="button" class="back" onclick={onBack}>{t("category.back")}</button>

  <h1>{categoryTitle(category, t)}</h1>
  <p class="definition">{categoryDefinition(category, t)}</p>

  {#if view === "loading"}
    <p class="state">{t("library.loading")}</p>
  {:else if error !== null}
    <p class="state error">{t("library.error", { message: error })}</p>
  {:else if view === "empty"}
    <p class="state">{t("home.categoryEmpty")}</p>
  {:else}
    <BookGrid
      entries={items}
      authors={authorSuggestions}
      {t}
      {onOpenFolder}
      {onOpenDocument}
    />
  {/if}
</section>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  h1 {
    margin: 0;
    font-size: 1.35rem;
  }

  .back {
    align-self: flex-start;
    padding: 0.35rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .back:hover {
    background: #f0f2f5;
  }

  .definition,
  .state {
    margin: 0;
    color: #5a6472;
  }

  .definition {
    font-size: 0.85rem;
  }

  .state.error {
    color: #a1301f;
  }
</style>
