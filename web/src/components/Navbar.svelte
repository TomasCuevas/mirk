<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import { screenLabel, screens, type Screen } from "../lib/screens";
  import type { NavMode } from "../lib/window";

  let {
    screen,
    mode,
    t,
    onSelect,
  }: {
    screen: Screen;
    /** Where the layout puts the navigation: the bottom bar, or full screen's left sidebar (S3). */
    mode: NavMode;
    t: Translate;
    onSelect: (screen: Screen) => void;
  } = $props();
</script>

<!--
  The navigation (S1). The current screen is marked twice on purpose: `aria-current="page"` for
  assistive technology and a visual treatment for everyone else, so which screen is active is never
  a guess.

  It is one list in two orientations (S3–S4 of `odd/tasks/navigation.md`): the bottom bar with the icon
  above the label, and — in full screen, where the width has room for it — a left sidebar with the icon
  before the label. Only `mode` changes the arrangement; the destinations, their order, their words and
  the active state are the same list either way, so the two modes cannot drift apart.
-->
<nav class={mode} aria-label={t("nav.ariaLabel")}>
  {#each screens as destination (destination)}
    <button
      type="button"
      class:active={destination === screen}
      aria-current={destination === screen ? "page" : undefined}
      onclick={() => onSelect(destination)}
    >
      <!-- Decorative: the label inside the button is the accessible name, so an icon that announced
           itself would say every destination twice. The hand is the top bar's — 24-unit box, round caps
           and joins — but the weight is lighter (L4): drawn at 22 px with the top bar's 1.6 the strokes
           read as heavy, so the same 24-unit box carries 1.2 and the rendered weight matches. -->
      <svg
        viewBox="0 0 24 24"
        width="22"
        height="22"
        fill="none"
        stroke="currentColor"
        stroke-width="1.2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        {#if destination === "home"}
          <path d="M4.4 10.9 12 4.5l7.6 6.4" />
          <path d="M6.6 9.9V19h10.8V9.9" />
        {:else if destination === "library"}
          <path d="M4.6 5.2h3.6v13.6H4.6z" />
          <path d="M9.8 5.2h3.6v13.6H9.8z" />
          <path d="m15.1 6.3 3.5-.7 2.2 12.9-3.5.6z" />
        {:else}
          <path d="M4.5 8.2h15" />
          <path d="M4.5 15.8h15" />
          <circle cx="9.2" cy="8.2" r="2.1" />
          <circle cx="14.8" cy="15.8" r="2.1" />
        {/if}
      </svg>
      <span class="label">{screenLabel(destination, t)}</span>
    </button>
  {/each}
</nav>

<style>
  /* The bar is canvas, not a card: the hairline is the only edge, and the active item is a lift of the
     surface rather than a filled accent. The accent belongs to the progress ramp, which is the only
     place a hue states a fact. */
  nav {
    flex: none;
    display: flex;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.045), rgba(255, 255, 255, 0.012)), var(--glass);
    backdrop-filter: blur(var(--glass-blur)) saturate(1.08);
    -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(1.08);
  }

  /* S2: the bottom bar is taller than it was. The label is the same size — what was too small was the
     target around it, so the bar gains padding and every item a 3.4 rem minimum, above the 44 px a
     pointer target is expected to have. */
  nav.bottom {
    gap: 0.2rem;
    padding: 0.3rem 0.5rem 0.35rem;
    border-top: 1px solid var(--glass-border);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
  }

  /* L16: the first round made the bar taller than it needed to be. The target stays comfortable (2.9 rem
     minimum, over the 44 px a pointer needs) and the height comes out of the air around it and out of the
     gap between the icon and its label, which are one thing and now sit as one. */
  nav.bottom button {
    flex: 1 1 0;
    min-width: 0;
    min-height: 2.9rem;
    flex-direction: column;
    gap: 0.1rem;
    padding: 0.25rem 0.35rem;
    font-size: var(--text-xs);
  }

  /* L15: in `rem`, not px, so the sidebar's drawings grow with the full-screen root instead of staying
     22 px beside 14.6 px words. */
  svg {
    width: 1.375rem;
    height: 1.375rem;
  }

  nav.bottom svg {
    width: 1.25rem;
    height: 1.25rem;
  }

  /* S3: full screen's left sidebar. It is a column of the shell, not an overlay, so it takes its width
     from the screen and leaves the rest to the listing. */
  /* L15: the pane keeps its visual width at the larger full-screen root (10.5 rem × 18 px ≈ 189 px,
     the 200 px it had at 16 px), which is also what leaves the grid the column it needs for a page of
     fourteen documents to fill two rows. */
  nav.side {
    flex-direction: column;
    width: 10.5rem;
    gap: 0.2rem;
    padding: 0.7rem 0.55rem;
    border-right: 1px solid var(--glass-border);
    box-shadow: inset 1px 0 0 rgba(255, 255, 255, 0.04);
    overflow-y: auto;
  }

  /* S4: in the sidebar the icon leads and the label follows, and the pair sits at the left edge — the
     shared row is centered for the bottom bar's column layout, where centering is what a stacked icon
     and label need. */
  nav.side button {
    width: 100%;
    min-height: 2.9rem;
    justify-content: flex-start;
    gap: 0.65rem;
    padding: 0.5rem 0.7rem;
    font-size: var(--text-sm);
    text-align: left;
  }

  button {
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--r-md);
    background: transparent;
    color: var(--fg-subtle);
    font: inherit;
    font-weight: var(--weight-medium);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  button:hover:not(.active) {
    color: var(--fg);
  }

  /* L5: the current destination is a pane of the wallpaper's mid-pink, not a white lift — the one place
     the image's colour is allowed to fill. The label stays near-white for contrast and the icon carries
     the brighter pink, so the state reads at a glance without becoming a button-shaped lamp. */
  button.active {
    background: linear-gradient(180deg, rgba(196, 122, 168, 0.26), rgba(196, 122, 168, 0.12));
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.12), 0 0 22px rgba(196, 122, 168, 0.18);
    color: var(--fg);
  }

  button.active svg {
    color: #f0bfd8;
  }

  button:focus-visible {
    outline: 2px solid var(--line-focus);
    outline-offset: 1px;
  }

  svg {
    flex: none;
  }

  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
