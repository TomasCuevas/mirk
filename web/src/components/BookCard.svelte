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
       placeholder repeats on every card and carries no information.

       The folder action sits on the cover's corner rather than in a row of its own. The first attempt
       gave it a row above the cover, and the user's verdict was that it looked loose up there with
       nothing to belong to (L27); anchored here it reads as an action on the document's own body, and
       it will still read that way once real covers replace the placeholder. -->
  <div class="cover-wrap">
    <div class="cover" aria-hidden="true">{t("card.cover")}</div>

    <!-- R9 (L22): an icon rather than a full-width text button. An icon is not a name, so the accessible
         name comes from the catalogue through `aria-label`, and the drawing is hidden from assistive
         technology because it repeats the name instead of adding to it. -->
    <button
      type="button"
      class="open-folder"
      aria-label={t("card.openFolder")}
      onclick={() => onOpenFolder(entry)}
    >
      <svg
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M3.2 7.4A1.6 1.6 0 0 1 4.8 5.8h4.1l1.9 2.1h8.4a1.6 1.6 0 0 1 1.6 1.6v6.9a1.6 1.6 0 0 1-1.6 1.6H4.8a1.6 1.6 0 0 1-1.6-1.6z" />
        <path d="M3.2 10.4h17.6" />
      </svg>
    </button>
  </div>

  <h2 class="title" title={displayTitle(entry)}>{displayTitle(entry)}</h2>

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

  <!-- The folder action carries no status text at all (L27, L30): the user removed the green "folder
       opened" sentence and then the failure sentence as well, so the card says nothing in either case.
       Opening the folder window is its own confirmation, and a failure is silent here — their call,
       recorded as theirs rather than dressed up as a design decision. -->
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    /* The card fills its grid column. Without this its width follows its own text, because a flex item
       does not grow by default — and a card with a shorter title came out **narrower**, which made its
       cover shorter too (the cover's height comes from its width, `aspect-ratio: 3/4`), pushed every
       line above the bar and left the hole the user reported twice. Measured in one row before the fix:
       covers of 186.2 and 146.0 px, a 40.2 px difference that had nothing to do with the gap (L29). */
    flex: 1;
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

  /* The action is positioned against the cover, which is what keeps it anchored to the document
     instead of sitting in a row of its own (L27). */
  .cover-wrap {
    position: relative;
    margin-bottom: 0.35rem;
  }

  .cover {
    display: flex;
    align-items: center;
    justify-content: center;
    aspect-ratio: 3 / 4;
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
    /* Cards in a row are stretched to the tallest one, and the difference between their contents has to
       go somewhere: that is the hole the user kept seeing in the shorter card (L28, L29). Pushing the
       progress block to the bottom moved the hole instead of shrinking it. Clamping the title to two
       lines is what shrinks it — to at most one line of difference — so a row reads as full. The text is
       only clipped visually: the element still holds the whole title for a screen reader, and the full
       title is offered to the pointer through `title=`. */
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
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
    /* A card is as tall as the tallest card in its row, and the difference is the title's line count.
       Pushing the progress block to the bottom spends that difference *inside* the card, instead of
       leaving it as dead space after the last line, and it lines every bar in a row up at the same
       height — which is the one comparison this screen exists to offer (L28). Measured in WebKitGTK
       4.1 before the change: a one-line title left 119.7 px of dead space in a 439.1 px card. */
    margin: auto 0 0;
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
    /* A fixed width in the pill's own font units, with a clamp inset of exactly half of it: the number
       is kept inside the bar at the ends **by construction**, rather than by a pixel value that has to
       be measured again every time the text size changes. `border-box` is what makes that half exact,
       and `1.7em`/`3.4em` scale with the font size above. */
    left: clamp(1.7em, var(--at), calc(100% - 1.7em));
    box-sizing: border-box;
    width: 3.4em;
    border: 1px solid #ccd2da;
    border-radius: 999px;
    background: #ffffff;
    color: #1c2024;
    font-size: 0.85rem;
    line-height: 1.5;
    text-align: center;
    white-space: nowrap;
    transform: translate(-50%, -50%);
  }

  .provenance {
    margin: 0;
    font-size: 0.75rem;
    color: #5a6472;
  }

  .open-folder {
    position: absolute;
    top: 0.3rem;
    right: 0.3rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.7rem;
    height: 1.7rem;
    padding: 0;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    cursor: pointer;
  }

  .open-folder:hover {
    background: #f0f2f5;
  }

</style>
