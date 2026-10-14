<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    describeLibraryMessage,
    filterEntries,
    paginate,
    sortEntries,
    sortKeys,
    type FolderActionStatus,
    type LibraryEntry,
    type SortKey,
  } from "../lib/library";
  import BookCard from "./BookCard.svelte";

  let {
    entries,
    loading,
    error,
    locale,
    t,
    folderStatus,
    onOpenFolder,
  }: {
    entries: LibraryEntry[];
    loading: boolean;
    error: string | null;
    locale: string;
    t: Translate;
    folderStatus: FolderActionStatus | null;
    onOpenFolder: (entry: LibraryEntry) => void;
  } = $props();

  // Filter, sort and page state is local to the screen, because it is a view of the listing and not a
  // property of the library. Filter and sort are pure functions over the listing already in hand
  // (S3/L19), and the page is cut from their result afterwards, so changing it cannot drop either.
  let query = $state("");
  let sort = $state<SortKey>("title");
  let page = $state(1);

  // `null` means there is something to list, so the empty-library sentence and the grid are
  // mutually exclusive rather than layered.
  const emptyMessage = $derived(describeLibraryMessage(entries, t));
  const visible = $derived(sortEntries(filterEntries(entries, query), sort, locale));
  // R10: the page is cut from the whole filtered, sorted listing, and the helper clamps the page into
  // range when a filter shrinks the listing under the reader.
  const paged = $derived(paginate(visible, page));
</script>

<section class="screen">
  <h1>{t("library.heading")}</h1>

  {#if loading}
    <p class="state">{t("library.loading")}</p>
  {:else if error !== null}
    <p class="state error">{t("library.error", { message: error })}</p>
  {:else if emptyMessage !== null}
    <p class="state">{emptyMessage}</p>
  {:else}
    <div class="controls">
      <label>
        <span>{t("library.filterLabel")}</span>
        <input type="search" bind:value={query} placeholder={t("library.filterPlaceholder")} />
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
      <p class="state">{t("library.noMatches", { query })}</p>
    {:else}
      <!-- Responsive columns: two at phone width (the shell is 390 px) and more if the window ever grows.
   The later phone-shape request wins over the earlier three-column one here, deliberately (S2). Each card reserves its cover space without pretending to have one. -->
      <!-- R10 (L22, L27): the page controls sit above the grid as well as below it, because otherwise
           reaching them costs a page of scrolling — the opposite of what pagination is for. They are one
           snippet rendered twice, so the two copies cannot drift apart. -->
      {@render pager()}

      <ul class="grid">
        {#each paged.items as entry (entry.id)}
          <li>
            <BookCard
              {entry}
              {t}
              {onOpenFolder}
              status={folderStatus?.id === entry.id ? folderStatus : null}
            />
          </li>
        {/each}
      </ul>

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
        onclick={() => (page = paged.page - 1)}
      >
        {t("library.previousPage")}
      </button>
      <span>{t("library.pagePosition", { page: paged.page, pages: paged.pageCount })}</span>
      <button
        type="button"
        disabled={paged.page >= paged.pageCount}
        onclick={() => (page = paged.page + 1)}
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

  /* One gap for both axes, which is what the design asked for from the start: the rows looked
     unseparated because the card overflowed its own grid row (see BookCard, where
     `box-sizing: border-box` is the line that fixes it), not because the gap was missing. The
     shorthand is honest again now that the real cause is fixed and measured. */
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(8.5rem, 1fr));
    column-gap: 0.75rem;
    row-gap: 0.75rem;
  }

  .grid li {
    display: flex;
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
