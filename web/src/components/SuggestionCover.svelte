<script lang="ts">
  /**
   * The cover of a suggested document and the two decisions that belong inside it (L60).
   *
   * Its own component for one reason: the preview is per document, so the state that says "the answer
   * has not arrived" belongs to one card rather than to the whole panel. It asks exactly once, on
   * mount. The panel re-reads the queue on the library's own timer, and a card that asked again on
   * every poll would render the same first page over and over for no new information.
   */
  import { onMount } from "svelte";
  import type { Translate } from "../lib/i18n";
  import { invoke } from "../lib/tauri";
  import {
    suggestionCover,
    type SuggestionCover as Cover,
  } from "../lib/suggestions";

  interface Props {
    id: number;
    path: string;
    t: Translate;
    deciding: boolean;
    onDecide: (id: number, accept: boolean) => Promise<void>;
  }

  let { id, path, t, deciding, onDecide }: Props = $props();

  let cover = $state<Cover>({ kind: "none" });

  onMount(() => {
    let cancelled = false;
    void invoke<string | null>("suggestion_cover", { path }).then(
      (src) => {
        if (!cancelled) {
          cover = suggestionCover({ status: "fulfilled", value: src });
        }
      },
      (error: unknown) => {
        if (!cancelled) {
          cover = suggestionCover({ status: "rejected", reason: error });
        }
      },
    );
    return () => {
      cancelled = true;
    };
  });
</script>

<div class="cover-wrap">
  {#if cover.kind === "shown"}
    <img
      class="cover-image"
      src={cover.src}
      alt=""
      style="aspect-ratio: 9 / 11"
    />
  {:else}
    <div class="cover" style="aspect-ratio: 9 / 11" aria-hidden="true">
      {t("card.cover")}
    </div>
  {/if}
  <div class="cover-decisions">
    <button
      type="button"
      class="accept"
      disabled={deciding}
      aria-label={t("suggestion.acceptLabel", { path })}
      onclick={() => onDecide(id, true)}
    >
      <svg
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M4 12.5 9.5 18 20 6.5" />
      </svg>
    </button>
    <button
      type="button"
      class="discard"
      disabled={deciding}
      aria-label={t("suggestion.discardLabel", { path })}
      onclick={() => onDecide(id, false)}
    >
      <svg
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M6.5 6.5 17.5 17.5M17.5 6.5 6.5 17.5" />
      </svg>
    </button>
  </div>
</div>

<style>
  .cover-wrap {
    position: relative;
  }

  /* The first page of a document the library has never seen (L60). The same rectangle and the same
     corner radius as a library cover, so a suggestion and a library document read as the same kind of
     object — which is the whole point of the card. The rules below are BookCard's, verbatim: if the two
     ever disagree, the two objects stop being one family. */
  .cover-image {
    display: block;
    width: 100%;
    box-sizing: border-box;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--surface-1);
    object-fit: contain;
  }

  /* The stand-in for a document whose first page could not be produced: the same rules the library
     card uses, so the two look identical rather than merely similar. */
  .cover {
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--surface-1);
    background-image: repeating-linear-gradient(
      45deg,
      transparent 0 9px,
      rgba(255, 255, 255, 0.018) 9px 18px
    );
    color: var(--fg-subtle);
    font-size: var(--text-xs);
    text-align: center;
  }

  /* The two decisions sit at the bottom of the cover's space and centred: the middle read as covering
     the document, and the bottom edge is where an action belongs.

     They are drawn as the card's own pills — the marker on the bar and the page badge on the cover — so
     the actions on a suggestion are the same kind of object as the facts on a library card: one raised
     surface, one strong hairline, fully rounded. */
  .cover-decisions {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 0.6rem;
    padding-bottom: 0.55rem;
  }

  .cover-decisions button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.4rem;
    /* `currentColor`, so each pill's edge is its own decision's colour and the two rules below only have
       to state the hue once. */
    border: 1px solid currentColor;
    border-radius: var(--r-full);
    background: var(--surface-2);
    cursor: pointer;
    transition: background var(--fast) var(--ease), border-color var(--fast) var(--ease);
  }

  .cover-decisions button:hover:not(:disabled) {
    background: var(--surface-3);
  }

  .cover-decisions button:disabled {
    cursor: default;
    opacity: 0.6;
  }

  /* Accept and discard keep their meaning in their hue — the same two colours the progress ramp starts
     and ends on — and the icon repeats it in words through the button's accessible name. */
  .accept {
    color: var(--success);
  }

  .discard {
    color: var(--danger);
  }
</style>
