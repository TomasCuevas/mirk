<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    describeProgressView,
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

  // The card's progress, as a fraction to draw and a number to place on it (R7).
  const view = $derived(describeProgressView(entry, t));
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

  <!-- R7: the bar stands for the fraction read and the number sits over it, at the position that
       fraction corresponds to. The provenance stays in words underneath, because the rule that
       progress is never stated as a bare percentage was the user's as well, and only the sentence for
       a document with no record at all was asked to go. -->
  <div class="progress">
    <div class="bar">
      <span class="fill" style="width: {view.fraction * 100}%"></span>
      <span class="marker" style="--at: {view.markerAt * 100}%">{view.marker}</span>
    </div>
    {#if view.provenance !== null}
      <p class="provenance">{view.provenance}</p>
    {/if}
  </div>

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
    /* `box-sizing: border-box` is what R8 turned out to need, and the reason is measurable. With the
       default `content-box`, `min-height: 100%` set the card's *content* box to the height of its grid
       row and then added the padding and the border on top of it: the card's border box stuck out
       24.4 px past its row (22.4 px of padding + 2 px of border), ate the 20 px row gap and overlapped
       the next row by 4.4 px. Reproduced and measured in WebKitGTK 4.1, the engine the application
       renders in: a row gap of -4.4 px before this line, exactly 20 px after it. The cover's
       `aspect-ratio` was the first suspect and measurements cleared it. */
    box-sizing: border-box;
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
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin: 0;
  }

  /* The number is placed on the bar itself, so the statement is the bar's own geometry rather than a
     caption beside it. The clamp is the honest compromise: a number has width, so at the very ends it
     is nudged inside the bar instead of hanging off it. **The inset is measured, not guessed**: the
     widest marker, "100 %", is 37 px across, so half of it is 18.5 px and the clamp uses 19.2 px
     (1.2rem). With 15.2 px the chip hung 3.3 px outside the bar at 100 %, which is what the
     measurement caught. */
  .bar {
    position: relative;
    height: 0.5rem;
    margin: 0.55rem 0;
    border-radius: 999px;
    background: #e8ebef;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: inherit;
    background: #24506b;
  }

  .marker {
    position: absolute;
    top: 50%;
    left: clamp(1.2rem, var(--at), calc(100% - 1.2rem));
    padding: 0 0.25rem;
    border: 1px solid #ccd2da;
    border-radius: 999px;
    background: #ffffff;
    color: #1c2024;
    font-size: 0.7rem;
    line-height: 1.5;
    white-space: nowrap;
    transform: translate(-50%, -50%);
  }

  .provenance {
    margin: 0;
    font-size: 0.75rem;
    color: #5a6472;
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
