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
     object — which is the whole point of the card. */
  .cover-image {
    display: block;
    width: 100%;
    box-sizing: border-box;
    border: 1px solid #e2e6eb;
    border-radius: 0.4rem;
    background: #ffffff;
    object-fit: contain;
  }

  /* The stand-in for a document whose first page could not be produced: the same rules the library
     card uses, so the two look identical rather than merely similar. */
  .cover {
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    border: 1px dashed #ccd2da;
    border-radius: 0.4rem;
    background: repeating-linear-gradient(45deg, #f6f7f9, #f6f7f9 8px, #eef0f3 8px, #eef0f3 16px);
    color: #8b95a3;
    font-size: 0.8rem;
    text-align: center;
  }

  /* The two decisions sit at the bottom of the cover's space and centred: the middle read as covering
     the document, and the bottom edge is where an action belongs. */
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
    border-radius: 999px;
    background: #ffffff;
  }

  .cover-decisions button:disabled {
    cursor: default;
    opacity: 0.6;
  }

  .accept {
    color: #1c6b3f;
    border-color: #1c6b3f;
  }

  .discard {
    color: #a1301f;
    border-color: #a1301f;
  }
</style>
