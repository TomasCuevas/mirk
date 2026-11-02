<script lang="ts">
  /**
   * S1 of `odd/tasks/suggestions-screen.md`: the queue of suggestions as a screen of its own.
   *
   * It is composed like Biblioteca — a heading, a count, one row of controls, a paginated grid of cards —
   * because a suggestion is the same kind of object as a library document: a cover and a name. What it
   * deliberately does NOT carry is any of Biblioteca's editing surface: no title field, no author chips,
   * no cover or page editor, and no command that would perform one (S6). The only two things the reader
   * can do with a suggestion are the two decisions that already existed (S7), and they sit on the card's
   * cover, where they have always been (`SuggestionCover`).
   *
   * Filter, order and page are local state, because they are a view of the queue and not a property of it
   * — the same reason Biblioteca keeps them here. Filtering and ordering are pure functions over the queue
   * already in hand (`filterSuggestions`, `sortSuggestions`), and the page is cut from their result with
   * the same `paginate` the library uses, so this screen cannot drift from that one about what a page is.
   */
  import type { Translate } from "../lib/i18n";
  import { pageWindow, paginate } from "../lib/library";
  import {
    describeHistoryFailure,
    describeNoSuggestionMatches,
    describeSuggestions,
    filterSuggestions,
    sortSuggestions,
    suggestionSorts,
    suggestionTitle,
    type DetectorBelief,
    type Suggestion,
    type SuggestionSort,
  } from "../lib/suggestions";
  import { libraryPageSize, type WindowLayout } from "../lib/window";
  import SuggestionCover from "./SuggestionCover.svelte";

  let {
    suggestions,
    readError,
    decisionError,
    historyError,
    detector,
    deciding,
    locale,
    layout,
    t,
    onDecide,
    onOpenFolder,
    onResetScroll,
    onBack,
  }: {
    /** The queue, or `null` until the store answers — "still reading" is not "nothing waiting". */
    suggestions: Suggestion[] | null;
    /**
     * The queue itself could not be read, or `null`. It is NOT the decision's failure and does not borrow
     * its words: a read that failed is not a decision that was attempted, so it has its own sentence
     * (`suggestion.readError`) rather than the one about a decision that was not recorded.
     */
    readError: string | null;
    /** A decision the reader made was not recorded, or `null`. This one keeps `suggestion.error`. */
    decisionError: string | null;
    /**
     * The once-per-run read of the desktop's own history that failed, or `null`. It is a fact about the
     * SOURCE and not about the queue, so it is shown beside the list rather than instead of it.
     */
    historyError: string | null;
    /** What the platform can say about open documents; only "unsupported" is a claim about the machine. */
    detector: DetectorBelief;
    /** True while a decision is being recorded, so neither button can be pressed twice. */
    deciding: boolean;
    locale: string;
    /** The window's layout, which decides the page size — the same count Biblioteca's page holds. */
    layout: WindowLayout;
    t: Translate;
    onDecide: (id: number, accept: boolean) => Promise<void>;
    /**
     * Opens the folder that holds a suggested document. It is the only thing the card's path can do: a
     * suggestion can be decided or located, and nothing about it can be edited (S6).
     */
    onOpenFolder: (path: string) => void;
    /**
     * Puts the shell's scroll back at the top when the page changes (S2, L73 of
     * `pdf-progress-tracker.md`). The shell owns the one scrolling container, and turning a page is not a
     * navigation, so the reset that answers navigation never fires without this.
     */
    onResetScroll: () => void;
    /**
     * Returns to Biblioteca (L10). The reader said it plainly: having to reach the bar to go back was the
     * missing half of arriving here. The destination is Biblioteca and not "the previous screen", because
     * that is the only screen that leads here and guessing would invent a history.
     */
    onBack: () => void;
  } = $props();

  // The default is the reader's own choice (S4): most recently opened first. The two alternatives are the
  // inverse and by title; there is no author control, because a suggestion has no authors (S3).
  let sort = $state<SuggestionSort>("recent");
  let query = $state("");
  let page = $state(1);

  // `null` means there is a list to render, so the message and the grid are mutually exclusive rather than
  // layered (L56). Three facts are kept apart: still reading, genuinely empty, and no detector at all.
  const message = $derived(describeSuggestions(suggestions, t, detector));
  const historyNotice = $derived(describeHistoryFailure(historyError, detector, t));

  // The pipeline Biblioteca uses, over the queue instead of the listing: filter, then order, then cut a
  // page out of the result — so no control can apply to a different set than the one on screen.
  const visible = $derived(
    sortSuggestions(filterSuggestions(suggestions ?? [], query), sort, locale),
  );
  const paged = $derived(paginate(visible, page, libraryPageSize(layout)));
  // A search that emptied the grid names what was typed. A blank one is not a failed search and says
  // nothing, which is what keeps the sentence from blaming the reader for a state they did not cause.
  const noMatches = $derived(describeNoSuggestionMatches(query, t));

  // One way to turn a page, so the scroll reset cannot be forgotten on one of the two pager copies.
  function turnTo(next: number): void {
    page = next;
    onResetScroll();
  }
</script>

<section class="screen">
  <!-- L10: the same way back the Ajustes sub-screens already have, because a screen reached from Biblioteca
       has to answer "how do I go back" without sending the reader to the bar. The arrow is decoration — the
       text is the label, and `aria-label` names the destination. -->
  <button type="button" class="back" aria-label={t("suggestions.backLabel")} onclick={onBack}>
    <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
        stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M12.5 8h-9" />
      <path d="M7.5 4 3.5 8l4 4" />
    </svg>
    {t("suggestions.back")}
  </button>

  <h1>{t("suggestion.heading")}</h1>
  <!-- R15: the count says something only when it says something new. "12 of 12 suggestions" is arithmetic
       dressed as information; the whole number is stated once, and the filtered fraction appears only when
       a search or a page actually hides suggestions. -->
  {#if readError === null && decisionError === null && suggestions !== null && suggestions.length > 0}
    <p class="lede">
      {paged.total === suggestions.length
        ? t("suggestions.countAll", { total: suggestions.length })
        : t("library.count", { shown: paged.total, total: suggestions.length })}
    </p>
  {/if}

  {#if readError !== null}
    <p class="state error">{t("suggestion.readError", { message: readError })}</p>
  {:else if decisionError !== null}
    <p class="state error">{t("suggestion.error", { message: decisionError })}</p>
  {:else if message !== null}
    <!-- The source failing is stated beside the state of the queue, never instead of it: a history that
         could not be read does not make an empty queue "nothing to decide". -->
    {#if historyNotice !== null}
      <p class="state error">{historyNotice}</p>
    {/if}
    <p class="state">{message}</p>
  {:else}
    {#if historyNotice !== null}
      <p class="state error">{historyNotice}</p>
    {/if}

    <div class="controls" class:row={layout === "auto"}>
      <label>
        <span>{t("suggestions.filterLabel")}</span>
        <input
          type="search"
          bind:value={query}
          placeholder={t("suggestions.filterPlaceholder")}
        />
      </label>
      <label>
        <span>{t("suggestions.sortLabel")}</span>
        <select bind:value={sort}>
          {#each suggestionSorts as key (key)}
            <option value={key}>{t(`suggestions.sort.${key}`)}</option>
          {/each}
        </select>
      </label>
    </div>

    {#if visible.length === 0}
      {#if noMatches !== null}
        <p class="state">{noMatches}</p>
      {/if}
    {:else}
      <!-- R10/L27 of Biblioteca: the page controls sit above and below the grid, because otherwise
           reaching them costs a page of scrolling. One snippet rendered twice, so the copies cannot
           drift apart. -->
      {@render pager()}

      <ul class="grid">
        {#each paged.items as suggestion (suggestion.id)}
          <li>
            <article class="card">
              <!-- L60: the cover and the two decisions are the existing piece, reused rather than
                   rebuilt — and they are the ONLY actions on the card, which is S6 stated in markup. -->
              <SuggestionCover
                id={suggestion.id}
                path={suggestion.path}
                {t}
                {deciding}
                onDecide={onDecide}
              />
              <div class="body">
                <!-- S5: the preset title, from the file's own name, set in a heading and never in a
                     field. The whole path stays readable for a pointer through `title=`. -->
                <h2 class="title" title={suggestion.path}>
                  {suggestionTitle(suggestion.path)}
                </h2>
                <!-- L58: the path is the card's context datum AND a control that opens the folder that
                     holds the file — a path a person cannot use is a dead line. -->
                <button
                  type="button"
                  class="path"
                  title={t("suggestion.openFolder", { path: suggestion.path })}
                  aria-label={t("suggestion.openFolder", { path: suggestion.path })}
                  onclick={() => onOpenFolder(suggestion.path)}
                >{suggestion.path}</button>
              </div>
            </article>
          </li>
        {/each}
      </ul>

      {@render pager()}
    {/if}
  {/if}

  <!-- S2: a pager with nothing to turn is not a control, it is noise — with one page `< 1 >` still
       claims there is more to see. The condition lives in the snippet rather than at its two call
       sites, so the copies cannot disagree. -->
  {#snippet pager()}
    {#if paged.pageCount > 1}
      <nav
        class="pager"
        class:right={layout !== "columns-2"}
        class:large={layout === "auto"}
        aria-label={t("suggestions.pagination")}
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
  .back {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    align-self: flex-start;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--surface-1);
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .back:hover {
    background: var(--surface-2);
    color: var(--fg);
  }

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

  .state.error {
    color: var(--danger);
  }

  .lede {
    margin: -0.35rem 0 0;
    color: var(--fg-muted);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }

  /* Biblioteca's own arrangement of the controls, minus the author selector this screen has no reason to
     offer (S3): the search owns its row and the order sits under it, and full screen puts the two side by
     side, where there is room for the query to be twice as wide as the selector. */
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

  .controls > label:first-of-type {
    flex: 1 0 100%;
  }

  .controls.row {
    flex-wrap: nowrap;
  }

  .controls.row > label:first-of-type {
    flex: 2 1 0;
  }

  /* L17: one control height for the whole application, tall enough to read and to hit. */
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

  /* S4 of the shell: the columns come from the layout the window is in — an exact count for a preset, or
     `auto-fill` at the card's own floor for full screen — so a suggestions page has the row Biblioteca's
     page has. The fallbacks are the grid's own rule, which is what a screen rendered on its own gets. */
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns:
      repeat(var(--grid-columns, auto-fill), minmax(var(--grid-card-min, 8.5rem), 1fr));
    column-gap: 0.6rem;
    row-gap: 0.6rem;
  }

  .card {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    flex-direction: column;
    gap: 0.4rem;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    min-width: 0;
  }

  /* Two lines at most, like a library card's title, so two suggestions never differ in height and the
     block under the cover keeps one shape down the row. The whole name is still there for a pointer
     through `title=` and for a screen reader.

     It is an `h2` and not a field: S6 forbids editing a suggestion's data, so the title exists only as
     text, and there is no control anywhere on this card that would change it. */
  .title {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    line-height: 1.2;
    color: var(--fg);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .path {
    background: none;
    border: 0;
    padding: 0;
    text-align: left;
    font-size: var(--text-xs);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
    cursor: pointer;
    transition: color var(--fast) var(--ease);
  }

  .path:hover {
    color: var(--fg);
    text-decoration: underline;
  }

  .pager {
    margin: 0.4rem 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.15rem;
    font-size: var(--text-sm);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .pager.right {
    justify-content: flex-end;
  }

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
