<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";
  import { layoutLabelKey, windowLayouts, type WindowLayout } from "../lib/window";

  let {
    layout,
    t,
    onLayout,
    onBack,
  }: {
    layout: WindowLayout;
    t: Translate;
    onLayout: (layout: WindowLayout) => void;
    /** Returns to the Ajustes index (R43/S2), through the same hash navigation as everything else. */
    onBack: () => void;
  } = $props();
</script>

<!--
  S5 of `odd/tasks/window-layout.md`. The Ventana sub-screen: how many documents a row holds, which is
  also how wide the window is.

  The four choices are rows rather than a dropdown for the reason R15 gives for the language: every
  choice is visible, and the one in force states itself by being the raised row instead of hiding the
  others behind a click. The sentence under them is what says the choice is not a grid preference — the
  window itself changes size, and the desktop process is what changes it.
-->
<section class="screen">
  <!-- R43/S2: a way back, named after the screen it returns to. -->
  <button type="button" class="back" aria-label={t("settings.backLabel")} onclick={onBack}>
    <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
        stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M12.5 8h-9" />
        <path d="M7.5 4 3.5 8l4 4" />
      </svg>
    {t("settings.back")}
  </button>

  <h1>{settingsSectionTitle("window", t)}</h1>
  <!-- R43/S4: one line saying what this screen is for. -->
  <p class="purpose">{settingsSectionPurpose("window", t)}</p>

  <div class="panel">
    <h2>{t("settings.window.heading")}</h2>
    <div class="choices" role="group" aria-label={t("settings.window.label")}>
      {#each windowLayouts as option (option)}
        <button
          type="button"
          class:current={layout === option}
          aria-pressed={layout === option}
          onclick={() => onLayout(option)}
        >
          {t(layoutLabelKey(option))}
        </button>
      {/each}
    </div>
    <p class="detail">{t("settings.window.hint")}</p>
  </div>
</section>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  h1 {
    margin: 0;
  }

  .back {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    align-self: flex-start;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--surface-1);
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .back:hover {
    background: var(--surface-2);
    color: var(--fg);
  }

  .purpose {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--fg-muted);
  }

  /* The block's name, as an eyebrow: small caps is what tells a reader this line names the block below
     it rather than starting a paragraph. */
  h2 {
    margin: 0 0 0.75rem;
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
    text-transform: uppercase;
    letter-spacing: var(--tracking-wide);
    color: var(--fg);
  }

  .panel {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.018));
    border: 1px solid var(--glass-border);
    border-radius: var(--r-lg);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
    padding: 0.8rem 0.9rem;
  }

  /* Four rows, not a dropdown: every layout is readable at once, and the one in force is the raised
     row — the same shape the language's segmented control uses, stretched to four choices. */
  .choices {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.2rem;
    background: var(--surface-1);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
  }

  .choices button {
    width: 100%;
    height: 2.4rem;
    padding: 0 0.6rem;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .choices button:hover:not(.current) {
    color: var(--fg);
  }

  .choices button.current {
    background: linear-gradient(180deg, rgba(196, 122, 168, 0.26), rgba(196, 122, 168, 0.12));
    border-color: rgba(196, 122, 168, 0.45);
    color: var(--fg);
    font-weight: var(--weight-semibold);
  }

  .choices button:focus-visible {
    outline: 2px solid var(--line-focus);
    outline-offset: 1px;
  }

  .detail {
    margin: 0.5rem 0 0;
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }
</style>
