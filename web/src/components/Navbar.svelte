<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import { screenLabel, screens, type Screen } from "../lib/screens";

  let {
    screen,
    t,
    onSelect,
  }: {
    screen: Screen;
    t: Translate;
    onSelect: (screen: Screen) => void;
  } = $props();
</script>

<!--
  The bottom navbar (S1). The current screen is marked twice on purpose: `aria-current="page"` for
  assistive technology and a visual treatment for everyone else, so which screen is active is never
  a guess.
-->
<nav aria-label={t("nav.ariaLabel")}>
  {#each screens as destination (destination)}
    <button
      type="button"
      class:active={destination === screen}
      aria-current={destination === screen ? "page" : undefined}
      onclick={() => onSelect(destination)}
    >
      {screenLabel(destination, t)}
    </button>
  {/each}
</nav>

<style>
  /* The navbar is the last child of the full-height shell, so it sits at the bottom of the fixed
     window without a fixed-position overlay or a padding hack. It is canvas, not a card: the hairline
     is the only edge, and the active item is a lift of the surface rather than a filled accent. The
     accent belongs to the progress ramp, which is the only place a hue states a fact. */
  nav {
    flex: none;
    display: flex;
    gap: 0.25rem;
    padding: 0.4rem 0.5rem;
    background: var(--canvas);
    border-top: 1px solid var(--line);
  }

  button {
    flex: 1 1 0;
    min-width: 0;
    padding: 0.45rem 0.4rem;
    border: none;
    border-radius: var(--r-md);
    background: transparent;
    color: var(--fg-subtle);
    font: inherit;
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  button:hover:not(.active) {
    color: var(--fg);
  }

  button.active {
    background: rgba(255, 255, 255, 0.07);
    color: var(--fg);
  }
</style>
