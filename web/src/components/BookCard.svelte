<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    describeEntryProgress,
    displayTitle,
    type FolderActionStatus,
    type LibraryEntry,
  } from "../lib/library";

  let {
    entry,
    t,
    status,
    onOpenFolder,
  }: {
    entry: LibraryEntry;
    t: Translate;
    status: FolderActionStatus | null;
    onOpenFolder: (entry: LibraryEntry) => void;
  } = $props();
</script>

<!--
  One library card (S2). The title is the embedded one or the file's own name — never the path.
  The progress is the shared sentence with its provenance in words, so the rule that holds on the
  library screen holds here too.
-->
<article>
  <!-- The cover's space is reserved and labelled. Covers are a later task (item 4), and a stand-in
       image would pretend to be something it is not. Hidden from assistive technology because the
       placeholder repeats on every card and carries no information. -->
  <div class="cover" aria-hidden="true">{t("card.cover")}</div>

  <h2 class="title">{displayTitle(entry)}</h2>

  {#if entry.author !== null}
    <p class="author">{t("library.author", { author: entry.author })}</p>
  {/if}

  <p class="pages">
    {entry.pages === null
      ? t("library.pagesUnknown")
      : t("library.pages", { count: entry.pages })}
  </p>

  <p class="progress">{describeEntryProgress(entry.progress, t)}</p>

  <button type="button" onclick={() => onOpenFolder(entry)}>{t("card.openFolder")}</button>

  {#if status !== null}
    {#if status.kind === "ok"}
      <p class="status">{t("card.openedFolder", { folder: status.folder, viewer: status.viewer })}</p>
    {:else}
      <p class="status error">{t("card.openFolderError", { message: status.message })}</p>
    {/if}
  {/if}
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    /* `min-height`, not `height`: the card follows its own content instead of being pinned to its grid
       row. Pinning it is the remaining suspect behind R8 — the rows of the library grid touching while
       the columns were properly separated (L22). */
    min-height: 100%;
    padding: 0.7rem;
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
  }

  .cover {
    display: flex;
    align-items: center;
    justify-content: center;
    aspect-ratio: 3 / 4;
    margin-bottom: 0.35rem;
    border: 1px dashed #ccd2da;
    border-radius: 0.4rem;
    background: repeating-linear-gradient(45deg, #f6f7f9, #f6f7f9 8px, #eef0f3 8px, #eef0f3 16px);
    color: #8b95a3;
    font-size: 0.8rem;
    text-align: center;
  }

  .title {
    margin: 0;
    font-size: 0.98rem;
    overflow-wrap: anywhere;
  }

  .author,
  .pages {
    margin: 0;
    font-size: 0.82rem;
    color: #5a6472;
    overflow-wrap: anywhere;
  }

  .progress {
    margin: 0;
    color: #24506b;
    font-size: 0.9rem;
  }

  button {
    margin-top: auto;
    padding: 0.4rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }

  button:hover {
    background: #f0f2f5;
  }

  .status {
    margin: 0;
    font-size: 0.78rem;
    color: #3c6e47;
    overflow-wrap: anywhere;
  }

  .status.error {
    color: #a1301f;
  }
</style>
