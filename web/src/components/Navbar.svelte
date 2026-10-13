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
  nav {
    position: fixed;
    inset: auto 0 0 0;
    display: flex;
    justify-content: center;
    gap: 0.25rem;
    padding: 0.5rem 1rem;
    background: #ffffff;
    border-top: 1px solid #e2e6eb;
    box-shadow: 0 -1px 6px rgba(28, 32, 36, 0.05);
  }

  button {
    min-width: 7rem;
    padding: 0.6rem 1rem;
    border: none;
    border-radius: 0.5rem;
    background: transparent;
    color: #5a6472;
    font: inherit;
    cursor: pointer;
  }

  button:hover {
    background: #f0f2f5;
  }

  button.active {
    background: #24506b;
    color: #ffffff;
    font-weight: 600;
  }
</style>
