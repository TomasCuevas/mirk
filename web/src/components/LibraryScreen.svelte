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
    pageWindow,
    paginate,
    sortEntries,
    sortKeys,
    type LibraryEntry,
    type SortKey,
  } from "../lib/library";
  import { libraryPageSize, type WindowLayout } from "../lib/window";
  import BookGrid from "./BookGrid.svelte";

  let {
    entries,
    inFlight,
    error,
    locale,
    layout,
    t,
    onOpenFolder,
    onOpenDocument,
    onResetScroll,
  }: {
    entries: LibraryEntry[];
    inFlight: boolean;
    error: string | null;
    locale: string;
    /** The window's layout, which decides the page size, the pager's place and the filters' row (L11). */
    layout: WindowLayout;
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
  // range when a filter shrinks the listing under the reader. L11: the cut is the row's own size, so a
  // page never ends halfway through a row.
  const paged = $derived(paginate(visible, page, libraryPageSize(layout)));

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

  {#if view !== "loading" && error === null && entries.length > 0}
    <!-- R15: the count says something only when it says something new. "42 of 42 documents" is
         arithmetic dressed as information; the whole number is stated once, and the filtered
         fraction appears only when a filter or a page actually hides documents. It sits under the
         title, where a subtitle belongs, rather than floating at the end of a row of controls. -->
    <p class="lede">
      {paged.total === entries.length
        ? t("library.countAll", { total: entries.length })
        : t("library.count", { shown: paged.total, total: entries.length })}
    </p>
  {/if}

  {#if view === "loading"}
    <p class="state">{t("library.loading")}</p>
  {:else if error !== null}
    <p class="state error">{t("library.error", { message: error })}</p>
  {:else if view === "empty"}
    <p class="state">{emptyMessage}</p>
  {:else}
    <div class="controls" class:row={layout === "auto"}>
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
    <!-- A pager with nothing to turn is not a control, it is noise: with one page the arrows are
         already disabled, and `< 1 >` above the grid still claims there is more to see. The snippet
         carries the condition itself rather than its two call sites, so the copies cannot disagree. -->
    {#if paged.pageCount > 1}
    <!-- R15: the reader asked for the familiar strip — arrows at the ends and the pages in between —
         instead of two sentences ("previous page", "next page") that made pagination the ugliest part
         of the screen. The numbers come from `pageWindow`, which decides which pages are worth
         showing; this snippet only draws what it decides. -->
    <nav
      class="pager"
      class:right={layout !== "columns-2"}
      class:large={layout === "auto"}
      aria-label={t("library.pagination")}
    >
      <button
        type="button"
        class="step"
        aria-label={t("library.previousPage")}
        disabled={paged.page <= 1}
        onclick={() => turnTo(paged.page - 1)}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
          stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M10 3.5 5.5 8l4.5 4.5" />
        </svg>
      </button>

      {#each pageWindow(paged.page, paged.pageCount) as slot, index (index)}
        {#if slot.kind === "gap"}
          <span class="gap" aria-hidden="true">…</span>
        {:else}
          <button
            type="button"
            class:current={slot.page === paged.page}
            aria-current={slot.page === paged.page ? "page" : undefined}
            aria-label={t("library.page", { page: slot.page })}
            onclick={() => turnTo(slot.page)}
          >{slot.page}</button>
        {/if}
      {/each}

      <button
        type="button"
        class="step"
        aria-label={t("library.nextPage")}
        disabled={paged.page >= paged.pageCount}
        onclick={() => turnTo(paged.page + 1)}
      >
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
          stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M6 3.5 10.5 8 6 12.5" />
        </svg>
      </button>
    </nav>
    {/if}
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
  }

  .state {
    margin: 0;
    color: var(--fg-muted);
  }

  .lede {
    margin: -0.35rem 0 0;
    color: var(--fg-muted);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }

  .state.error {
    color: var(--danger);
  }

  /* The controls sit on the canvas rather than in a panel of their own, and they arrange themselves in
     three rows without the markup saying so: the text filter is the whole first row, the author and the
     order share the second, and the count is a line of its own under them. `align-items: flex-end` keeps
     the two selects on the same baseline as the search field above them. */
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2);
  }

  .controls label {
    display: flex;
    flex: 1 1 0;
    min-width: 0;
    flex-direction: column;
    gap: 0.25rem;
    font-size: var(--text-xs);
    color: var(--fg-subtle);
  }

  /* The search label owns its row: a query is a sentence, and half a window is not enough to type one. */
  .controls > label:first-of-type {
    flex: 1 0 100%;
  }

  /* L11: full screen has the width for the three filters side by side, which saves the two extra rows
     the stacked layout costs — the height a maximised listing would rather give to documents. The query
     keeps twice the room of each selector, because it is the one a reader types into. */
  .controls.row {
    flex-wrap: nowrap;
  }

  .controls.row > label:first-of-type {
    flex: 2 1 0;
  }

  /* L17: one control height for the whole application, tall enough to read and to hit. It is a single
     value because the search field and the two selectors are one row of one toolbar. */
  input,
  select {
    height: 2.5rem;
    padding: 0 0.55rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    background-color: rgba(255, 255, 255, 0.06);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--fg);
    box-sizing: border-box;
    max-width: 100%;
  }

  input::placeholder {
    color: var(--fg-subtle);
  }

  input:focus-visible,
  select:focus-visible {
    outline: none;
    border-color: var(--line-focus);
    box-shadow: 0 0 0 3px rgba(169, 184, 240, 0.16);
  }

  input {
    width: 100%;
    min-width: 0;
  }

  .pager {
    /* R16: a little air above and below the strip. Without it the page numbers were glued to the
       grid above and to the bottom edge of the screen. */
    margin: 0.4rem 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.15rem;
    font-size: var(--text-sm);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  /* L11: from three per row the grid fills the window, so the strip follows the content and ends at the
     right edge instead of floating in the middle of a wide screen. */
  .pager.right {
    justify-content: flex-end;
  }

  /* Full screen's controls are read from farther away, so its page numbers and arrows are larger. */
  .pager.large {
    gap: 0.25rem;
    font-size: var(--text-base);
  }

  .pager.large button {
    min-width: 2.4rem;
    height: 2.4rem;
    padding: 0 0.5rem;
    font-size: var(--text-sm);
  }

  .pager.large .gap {
    min-width: 1.4rem;
  }

  /* A page number is a quiet button until it is the page you are on; the current one is a filled
     pill, which is the only thing in the strip that carries weight. */
  .pager button {
    min-width: 1.9rem;
    height: 1.9rem;
    padding: 0 0.4rem;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .pager button:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--fg);
  }

  .pager button.current {
    background: var(--brand-mid);
    color: var(--fg-on-loud);
    font-weight: var(--weight-semibold);
    box-shadow: 0 0 16px rgba(196, 122, 168, 0.22);
  }

  .pager button.step {
    color: var(--fg-subtle);
  }

  .pager .gap {
    min-width: 1.1rem;
    text-align: center;
    color: var(--fg-subtle);
  }

  .pager button:disabled {
    color: var(--fg-subtle);
    opacity: 0.45;
    cursor: default;
  }
</style>
