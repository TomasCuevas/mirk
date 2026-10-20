<script lang="ts">
  import { onMount } from "svelte";
  import type { Translate } from "../lib/i18n";
  import {
    coverSource,
    describeAuthors,
    describeProgressView,
    displayTitle,
    fetchCover,
    type LibraryEntry,
  } from "../lib/library";
  import { invoke } from "../lib/tauri";

  let {
    entry,
    t,
    onOpenFolder,
    onOpenDocument,
    onEditDocument,
    cover,
  }: {
    entry: LibraryEntry;
    t: Translate;
    onOpenFolder: (entry: LibraryEntry) => void;
    /**
     * Opens the document itself, in the system's own application for it (R37).
     *
     * The cover and the title are the two ways in: the cover is the document's body and the title
     * is its name, and both say the same thing — "this is the document, open it". The card's other
     * two actions (the folder and the editor) are about the document's surroundings and its
     * metadata, and they keep saying so.
     */
    onOpenDocument: (entry: LibraryEntry) => void;
    /** Opens the document editor for this entry: its title and its cover, in the one modal (R32). */
    onEditDocument: (entry: LibraryEntry) => void;
    /**
     * The cover the caller already has, when it has one. The grid does not pass it — a card asks the
     * application for its own cover on mount (R32) — but a caller that already knows a cover avoids
     * the round trip, and a test can render the card with one without a webview.
     */
    cover?: string | null;
  } = $props();

  // The cover the card asked for on mount (R32). It stays `null` until the application answers with
  // an image, so a missing cover and a cover still being produced both leave the placeholder up.
  let fetchedCover = $state<string | null>(null);
  const imageSource = $derived(coverSource(cover === undefined ? fetchedCover : cover));

  onMount(() => {
    if (cover !== undefined) {
      // The caller already knows: there is nothing to ask for.
      return;
    }
    // `live` stops a late answer from writing into a card that has already been torn down, which
    // would be a state update after unmount rather than a cover nobody would ever see.
    let live = true;
    void fetchCover(entry, invoke).then((source) => {
      if (live) {
        fetchedCover = source;
      }
    });
    return () => {
      live = false;
    };
  });

  // The card's progress, as a fraction to draw and a number to place on it (R7).
  const view = $derived(describeProgressView(entry, t));

  // The authors as a sentence a person reads (R42/S4): "by Carl Sagan y Ann Druyan" for two names,
  // commas and one connector for three or more. The list is the one the core resolved — the reader's
  // when they set one, otherwise the document's own — and `null` means nobody is named, so the card
  // says nothing rather than printing the words "by" with no name after them.
  const authors = $derived(describeAuthors(entry.authors, t));
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
    <!-- The cover is the document's own body, and clicking it opens the document (R37).
         It is a real button, so the pointer and the keyboard reach the same action, and the
         accessible name names the DOCUMENT: on a screen that holds dozens of cards, a control that
         only says "open" says nothing about which one it opens.
         The two corner pills stay outside it — they are their own actions, and a button inside a
         button is not a thing. -->
    <button
      type="button"
      class="cover-open"
      aria-label={t("card.openDocument", { title: displayTitle(entry) })}
      onclick={() => onOpenDocument(entry)}
    >
      {#if imageSource !== null}
        <!-- The document's real cover (R32). Its shape matches the placeholder's (`aspect-ratio` is
             declared inline on both), so a card does not change height when the image arrives; and
             `object-fit: contain` shows the whole page rather than cropping the part of it that
             carries a title. The `alt` is empty because the heading below already names the
             document: a second copy of the same name would be read out twice. -->
        <img class="cover-image" src={imageSource} alt="" style="aspect-ratio: 9 / 11" />
      {:else}
        <div class="cover" style="aspect-ratio: 9 / 11" aria-hidden="true">{t("card.cover")}</div>
      {/if}
    </button>

    <!-- R31, corrected in R32: the document-editor pencil sits on the cover's TOP-LEFT, mirroring the
         containing-folder action at its top-right, because they are the same kind of thing: an action
         on the document's own body, anchored to the space that stands for it. It is the ONE editing
         surface: the title and the cover both live in the modal it opens, which is why a cover control
         of its own was removed — a third control over a small cover was the user's complaint. An icon
         is not a name, so the accessible name comes from the catalogue and names the document; the
         drawing is hidden from assistive technology because it repeats that name rather than adding to
         it. -->
    <button
      type="button"
      class="edit-document"
      aria-label={t("card.editDocument", { title: displayTitle(entry) })}
      onclick={() => onEditDocument(entry)}
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
        <path d="M4 20h4.2L19 9.2a2 2 0 0 0 0-2.8l-1.4-1.4a2 2 0 0 0-2.8 0L4 15.8z" />
        <path d="M13.6 6.8l3.6 3.6" />
      </svg>
    </button>

    <!-- R19: the page count sits on the cover's bottom-right, mirroring the folder action at its
         top-right, so the two corners read as one design instead of two. It says nothing when the
         count is unknown: a corner badge cannot hold "Número de páginas desconocido", and a card that
         cannot state a number must not invent one. It is the truth, not an action, and it stays. -->
    {#if entry.pages !== null}
      <p class="cover-pages">{t("library.pages", { count: entry.pages })}</p>
    {/if}

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

  <!-- The text block below the cover is a FIXED height, and that is the whole point: it is what makes
       a tile with a real document and the empty placeholder end up exactly the same size, which is
       the user's stated requirement. The title is clamped to two lines and two lines are reserved
       here, so a one-line and a two-line title occupy the same slot; the progress block is pushed to
       the bottom of the fixed block by its own `margin: auto 0 0`, so every bar sits at one height.
       The title is only clipped visually — the element still holds the whole title for a screen
       reader and offers it to the pointer through `title=`.

       The height above is computed for exactly what this block contains, which is why the page count
       is rendered here and nowhere else. It used to be rendered a second time above the cover as
       well (R18): the same sentence twice on one card, and — because that copy sat outside this fixed
       block — a card pushed past its own height, whose overflow then made the carousel strip scroll
       vertically (R17). A fixed height its own content overflows is not fixed. -->
  <div class="body">
    <!-- The title opens the document as well (R37): it is the document's name, and a name a reader
         recognises is the thing they reach for. The clamp lives on the SPAN inside the button and
         not on the button itself, because a form control is a replaced element in some engines and
         a line clamp on one is not honoured; on a plain span it is. The heading still holds the
         title for a screen reader, and the button's own `title` offers the full name to the
         pointer. -->
    <h2 class="title">
      <button
        type="button"
        class="title-open"
        title={displayTitle(entry)}
        onclick={() => onOpenDocument(entry)}
      >
        <span class="title-text">{displayTitle(entry)}</span>
      </button>
    </h2>

    <!-- The card used to print "your title" here when the reader had named the document, and the user
         asked for it to go: on a title they had just typed themselves it was noise, not information.
         Provenance is still stated, in the one place where it is worth stating — the editor says which
         title is being edited and offers to restore the document's own only when there is one to restore
         — and `titleEditor` in `library.ts` is where that rule now lives and is tested. -->

    {#if authors !== null}
      <p class="author">{t("library.author", { author: authors })}</p>
    {/if}

    <!-- R7: the bar stands for the fraction read and the number sits over it, at the position that
         fraction corresponds to. The provenance stays in words underneath, because the rule that
         progress is never stated as a bare percentage was the user's as well, and only the sentence for
         a document with no record at all was asked to go.

         R38: the FILL travels a continuous colour ramp as the reading advances — from red through
         orange and yellow to green, slowly enough that two neighbouring documents differ by a shade
         rather than by a state — and the bar itself keeps the neutral track it always had. The colour
         is a second channel, never the only one: the number on the bar states the percentage, so a
         reader who cannot tell the hues apart loses nothing. -->
    <div class="progress">
      <div class="bar">
        <span class="fill" style="width: {view.fraction * 100}%; background: {view.color}"></span>
        <span class="marker" style="--at: {view.markerAt * 100}%">{view.marker}</span>
      </div>
      {#if view.provenance !== null}
        <p class="provenance">{view.provenance}</p>
      {/if}
    </div>
  </div>

  <!-- The folder action carries no status text at all (L27, L30): the user removed the green "folder
       opened" sentence and then the failure sentence as well. The card therefore receives no outcome
       and renders none — the two sentences' plumbing is gone with them. Opening the folder window is
       its own confirmation, and a failure is reported to the console by the application; their call,
       recorded as theirs rather than dressed up as a design decision. -->
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    /* The card fills its grid column. Without this its width follows its own text, because a flex item
       does not grow by default — and a card with a shorter title came out **narrower**, which made its
       cover shorter too (the cover's height comes from its width, `aspect-ratio`), pushed every line
       above the bar and left the hole the user reported twice. Measured in one row before the fix, at
       the then-current 3/4 ratio: covers of 186.2 and 146.0 px, a 40.2 px difference that had nothing
       to do with the gap (L29). */
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
     instead of sitting in a row of its own (L27). No bottom margin: the article's `gap` already
     separates the cover from the text block, and the empty placeholder uses the same single gap so the
     two stay the same height. */
  .cover-wrap {
    position: relative;
  }

  /* The cover takes the 9/11 ratio the user asked for, and it applies here for the library too — one
     card, one shape. The ratio is declared inline on the element rather than in this rule, so the
     empty placeholder in `HomeScreen` can carry the identical declaration and a test can prove the
     two are the same shape without a browser. */
  .cover {
    display: flex;
    align-items: center;
    justify-content: center;
    /* `border-box` so the 9/11 ratio describes the visible cover rectangle, border included, rather
       than its content box. The placeholder sets the same so the two rectangles are identical. */
    box-sizing: border-box;
    border: 1px dashed #ccd2da;
    border-radius: 0.4rem;
    background: repeating-linear-gradient(45deg, #f6f7f9, #f6f7f9 8px, #eef0f3 8px, #eef0f3 16px);
    color: #8b95a3;
    font-size: 0.8rem;
    text-align: center;
  }

  /* R32: the real cover occupies exactly the placeholder's rectangle. `width` and the inline
     `aspect-ratio` fix the box; `object-fit: contain` letterboxes a page of any ratio inside it
     instead of cropping it, and the white background is what shows in the letterbox. */
  .cover-image {
    display: block;
    width: 100%;
    box-sizing: border-box;
    border: 1px solid #e2e6eb;
    border-radius: 0.4rem;
    background: #ffffff;
    object-fit: contain;
  }

  /* The fixed height of the text block. Like the cover ratio, the height is declared inline on the
     element so the placeholder can declare the identical 11rem and a test can compare the two. The
     value fits the tallest realistic content: two title lines (2.94rem), an author line (1.23rem),
     the page line (1.23rem), the progress block with a TWO-line provenance (bar 1.6 + gap 0.25 +
     2×1.125 = 4.1rem) and three 0.35rem gaps (1.05rem) = 10.55rem, rounded up to 11rem. A one-line
     provenance — `recorded by oct`, `recorded by Evince` — simply leaves slack at the top, which the
     progress block's `margin: auto 0 0` spends before the bar. The reserve is two provenance lines
     rather than one because a close-time viewer says `recorded when Evince closed`, which wraps at
     this width; a fixed height that a real string can overflow is not fixed.

     The page line is no longer one of the things this height has to hold — it moved onto the cover as
     a badge (R19) — so the block now carries that line's worth of slack. The value stays 11rem on
     purpose: shrinking it would make the placeholder's identical declaration wrong, and the slack is
     the headroom the optional author line already needed. */
  .body {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  /* The cover's own control (R37). The button owns no shape of its own: it is the cover's
     rectangle, so the cover keeps exactly the geometry it had before the click target arrived —
     the same 9/11 rectangle, the same border, the same stand-in. Everything that would draw a
     button is removed, because a visible button frame around the cover would say "control" where
     the cover says "document". */
  .cover-open {
    display: block;
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
  }

  .title {
    margin: 0;
    font-size: 0.98rem;
    overflow-wrap: anywhere;
    /* The title does not make every card the same height — the fixed text block that did that was
       removed at the user's request, because reserving it made the card read as stretched and left
       the text floating in the space it created (L31). The text is only clipped visually: the
       element still holds the whole title for a screen reader, and the full title is offered to
       the pointer through `title=`. */
  }

  /* The title as a control (R37). It looks like the title it always was — no frame, no underline
     on its own, the same colour and size — and the pointer says so with the cursor alone, which is
     the one signal that costs nothing on a screen full of cards. */
  .title-open {
    display: block;
    width: 100%;
    margin: 0;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  /* The two-line clamp, on the span and not on the button: a form control is a replaced element
     in some engines, and `-webkit-line-clamp` on one is not honoured the way it is on a plain
     element. The span inside the button is a plain element, so the clamp is the same clamp the
     title had before it became a control. */
  .title-text {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }

  .author,
  .progress {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    /* Inside the fixed-height body, pulling the bar to the bottom puts it at the same height in every
       tile — the one comparison a progress screen exists to offer (L28). Before the body had a fixed
       height this absorbed the title's line-count difference; now it simply anchors the bar to the
       bottom of the reserved block. */
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

  /* R38: the fill's colour is computed from the fraction and arrives as an inline `background`, so it
     travels from red to green through orange and yellow in one continuous ramp instead of switching
     between four states. The `background` above in `.fill` stays as the fallback for a caller that
     builds a view without a colour: a fill that lost its background would be an invisible bar, and an
     invisible bar says "nothing read" about a document that was read. The track keeps its own neutral
     grey — the user asked for the progress to change colour, not the bar. */

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

  /* R31, corrected in R32: the same pill as the folder action, on the opposite corner, and the ONE
     editing surface — it opens the modal that holds both the title and the cover. */
  .edit-document {
    position: absolute;
    top: 0.3rem;
    left: 0.3rem;
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

  .edit-document:hover {
    background: #f0f2f5;
  }

  /* R31: the reader's-title note. Muted and small, because it marks whose title this is rather than
     restating it. */
  /* R19: the page count as a badge on the cover's bottom-right. Same inset, border radius and white
     pill as the folder action one line up, because they are the same kind of thing: a small fact
     about the document, stated on the space that stands for its body. */
  .cover-pages {
    position: absolute;
    bottom: 0.3rem;
    right: 0.3rem;
    margin: 0;
    padding: 0.15rem 0.4rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #55606d;
    font-size: 0.72rem;
  }

</style>
