<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    ALL_AUTHORS,
    authorFilter,
    describeLibraryMessage,
    describeNoMatches,
    filterAuthors,
    filterByAuthor,
    filterEntries,
    libraryAuthors,
    libraryView,
    paginate,
    sortEntries,
    sortKeys,
    type LibraryEntry,
    type SortKey,
  } from "../lib/library";
  import BookGrid from "./BookGrid.svelte";

  let {
    entries,
    inFlight,
    error,
    locale,
    t,
    onOpenFolder,
    onOpenDocument,
    onResetScroll,
  }: {
    entries: LibraryEntry[];
    inFlight: boolean;
    error: string | null;
    locale: string;
    t: Translate;
    onOpenFolder: (entry: LibraryEntry) => void;
    /** Opens the document itself, in the system's own application for it (R37). */
    onOpenDocument: (entry: LibraryEntry) => void;
    /**
     * Puts the shell's scroll back at the top, which the screen asks for when it turns a page (L73).
     *
     * The shell owns the one scrolling container — the screen between the top bar and the navbar — so a
     * screen cannot reset it itself, and turning a page is not a navigation: `view` does not change, so
     * the reset that answers navigation never fires. Without this, page two of the library opens
     * wherever page one was scrolled to, which is the report that opened this round.
     */
    onResetScroll: () => void;
  } = $props();

  // Filter, sort and page state is local to the screen, because it is a view of the listing and not a
  // property of the library. Filter and sort are pure functions over the listing already in hand
  // (S3/L19), and the page is cut from their result afterwards, so changing it cannot drop either.
  let query = $state("");
  let sort = $state<SortKey>("title");
  let page = $state(1);

  // R39. The author the reader picked, held as the selector's own value, where `ALL_AUTHORS` means
  // "all". What the screen actually filters by is *derived* from the authors the cards show right now
  // (`chosenAuthor`), which is how a chosen author who leaves the library falls back to "all" on the
  // next render, silently and without an effect watching for it (S4).
  let authorChoice = $state(ALL_AUTHORS);
  const authors = $derived(filterAuthors(entries, locale));
  const chosenAuthor = $derived(authorFilter(authors, authorChoice));
  // R40/S1. The editor's own list is a different rule over a different scope: every author the reader
  // has used, including their own, computed from the WHOLE listing — never from `paged.items`, which is
  // the ten documents this page holds and is exactly how an author on page two stopped existing.
  const authorSuggestions = $derived(libraryAuthors(entries, locale));

  // `null` means there is something to list, so the empty-library sentence and the grid are
  // mutually exclusive rather than layered.
  const emptyMessage = $derived(describeLibraryMessage(entries, t));
  // L47. One decision for what the screen shows, shared with Inicio and pinned without rendering: an
  // empty listing is only presented AS empty once nothing is in flight. The loader is the catalogue's
  // existing loading sentence, which is as true of a rescan as of the first read.
  const view = $derived(libraryView(entries, inFlight));
  // R39/S3: text first, then author, then order — one pipeline over the whole listing, so no two of
  // the three controls can disagree about what is on screen.
  const visible = $derived(
    sortEntries(filterByAuthor(filterEntries(entries, query), chosenAuthor), sort, locale),
  );
  // R10: the page is cut from the whole filtered, sorted listing, and the helper clamps the page into
  // range when a filter shrinks the listing under the reader.
  const paged = $derived(paginate(visible, page));

  // One way to turn a page, so the scroll reset cannot be forgotten on one of the two controls — the
  // pager is rendered twice (above and below the grid) and both go through here.
  function turnTo(next: number): void {
    page = next;
    onResetScroll();
  }
  // R39/S5: the sentence for a listing filtered into nothing names what was filtered — the query when
  // there is text, the chosen author when there is not, and nothing at all when neither was set.
  const noMatches = $derived(describeNoMatches(query, chosenAuthor, t));
</script>

<section class="screen">
  <h1>{t("library.heading")}</h1>

  {#if view === "loading"}
    <p class="state">{t("library.loading")}</p>
  {:else if error !== null}
    <p class="state error">{t("library.error", { message: error })}</p>
  {:else if view === "empty"}
    <p class="state">{emptyMessage}</p>
  {:else}
    <div class="controls">
      <label>
        <span>{t("library.filterLabel")}</span>
        <input type="search" bind:value={query} placeholder={t("library.filterPlaceholder")} />
      </label>
      <!-- R39/S1: a third control, beside the text filter and the sort — neither replaces the other two.
           It lists the authors the cards show plus an "all" entry, and it is bound one way to the
           RESOLVED choice so a value that left the library cannot stay in the box while the grid
           behind it lists everybody (S4). -->
      <label>
        <span>{t("library.authorLabel")}</span>
        <select
          value={chosenAuthor}
          onchange={(event) => (authorChoice = event.currentTarget.value)}
        >
          <option value={ALL_AUTHORS}>{t("library.authorAll")}</option>
          {#each authors as author (author)}
            <option value={author}>{author}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>{t("library.sortLabel")}</span>
        <select bind:value={sort}>
          {#each sortKeys as key (key)}
            <option value={key}>{t(`library.sort.${key}`)}</option>
          {/each}
        </select>
      </label>
      <span class="count">{t("library.count", { shown: paged.total, total: entries.length })}</span>
    </div>

    {#if visible.length === 0}
      {#if noMatches !== null}
        <p class="state">{noMatches}</p>
      {/if}
    {:else}
      <!-- Responsive columns: two at phone width (the shell is 390 px) and more if the window ever grows.
   The later phone-shape request wins over the earlier three-column one here, deliberately (S2). Each card reserves its cover space without pretending to have one. -->
      <!-- R10 (L22, L27): the page controls sit above the grid as well as below it, because otherwise
           reaching them costs a page of scrolling — the opposite of what pagination is for. They are one
           snippet rendered twice, so the two copies cannot drift apart. -->
      {@render pager()}

      <BookGrid
        entries={paged.items}
        authors={authorSuggestions}
        {t}
        {onOpenFolder}
        {onOpenDocument}
      />

      {@render pager()}
    {/if}
  {/if}
  <!-- One definition of the page controls, rendered above and below the grid (R10, L27). The buttons
       move the window over the same filtered, sorted listing, so turning a page keeps the filters. -->
  {#snippet pager()}
    <div class="pager">
      <button
        type="button"
        disabled={paged.page <= 1}
        onclick={() => turnTo(paged.page - 1)}
      >
        {t("library.previousPage")}
      </button>
      <span>{t("library.pagePosition", { page: paged.page, pages: paged.pageCount })}</span>
      <button
        type="button"
        disabled={paged.page >= paged.pageCount}
        onclick={() => turnTo(paged.page + 1)}
      >
        {t("library.nextPage")}
      </button>
    </div>
  {/snippet}
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

  .state {
    margin: 0;
    color: #5a6472;
  }

  .state.error {
    color: #a1301f;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 0.6rem 1rem;
    padding: 0.7rem 0.8rem;
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
  }

  .controls label {
    display: flex;
    flex: 1 1 8rem;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
    color: #5a6472;
  }

  input,
  select {
    padding: 0.45rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    font: inherit;
    font-size: 0.9rem;
    color: #1c2024;
    box-sizing: border-box;
    max-width: 100%;
  }

  input {
    width: 100%;
    min-width: 0;
  }

  .count {
    margin-left: auto;
    font-size: 0.8rem;
    color: #5a6472;
  }

  .pager {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.6rem;
    font-size: 0.85rem;
    color: #5a6472;
  }

  .pager button {
    padding: 0.4rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .pager button:hover:not(:disabled) {
    background: #f0f2f5;
  }

  .pager button:disabled {
    color: #9aa3af;
    cursor: default;
  }
</style>
