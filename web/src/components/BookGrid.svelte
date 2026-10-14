<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import type { FolderActionStatus, LibraryEntry } from "../lib/library";
  import BookCard from "./BookCard.svelte";

  let {
    entries,
    t,
    folderStatus,
    onOpenFolder,
    layout = "grid",
  }: {
    entries: readonly LibraryEntry[];
    t: Translate;
    folderStatus: FolderActionStatus | null;
    onOpenFolder: (entry: LibraryEntry) => void;
    layout?: "grid" | "strip";
  } = $props();
</script>

<!--
  The one place cards are wired up (B2). The library, a category's full list and Inicio's carousels
  all render this, so "the same cards as the library" is a fact of the code rather than three copies
  of the same markup that can drift. `layout` only changes how the list is laid out: a wrapping grid
  for a full list, a horizontal scrolling strip for a carousel (B1).
-->
<ul class="grid" class:strip={layout === "strip"}>
  {#each entries as entry (entry.id)}
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

<style>
  /* One gap for both axes, which is what the design asked for from the start: the rows looked
     unseparated because the card overflowed its own grid row (see BookCard, where
     `box-sizing: border-box` is the line that fixes it), not because the gap was missing. */
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

  /* B1: a carousel scrolls sideways rather than wrapping. The tiles keep a fixed width so the strip
     has a definite end to scroll to, and `overflow-x: auto` is the scroll itself. */
  .grid.strip {
    display: flex;
    flex-wrap: nowrap;
    overflow-x: auto;
    gap: 0.75rem;
    padding-bottom: 0.35rem;
  }

  .grid.strip li {
    /* The tile width is the carousel's own, inherited as `--tile`, so a card and an empty slot are
       the same size by construction. The fallback covers a strip used without that variable. */
    flex: 0 0 var(--tile, 8.5rem);
  }
</style>
