<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    describeProvider,
    describeProviderRegistry,
    type Provider,
  } from "../lib/library";
  import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";

  let {
    providers,
    providerError,
    t,
    onBack,
  }: {
    providers: Provider[] | null;
    providerError: string | null;
    t: Translate;
    /** Returns to the Ajustes index (R43/S2), through the same hash navigation as everything else. */
    onBack: () => void;
  } = $props();

  const providerMessage = $derived(describeProviderRegistry(providers, t));
</script>

<!--
  R43/S3 (L71). The Lectura sub-screen: the progress providers, and the place a future reading
  preference belongs.

  It holds one block today and that is honest rather than thin: a reading preference is exactly what
  this page is for, and the providers are the only one that exists. Each provider states what it is and
  either that this machine can use it or why it cannot — the three states the registry keeps apart.
-->
<section class="screen">
  <!-- R43/S2: a way back, named after the screen it returns to. -->
  <!-- R15: the arrow is the affordance; the label names where it goes. -->
  <button
    type="button"
    class="back"
    aria-label={t("settings.backLabel")}
    onclick={onBack}
  >
    <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
        stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M12.5 8h-9" />
        <path d="M7.5 4 3.5 8l4 4" />
      </svg>
    {t("settings.back")}
  </button>

  <h1>{settingsSectionTitle("reading", t)}</h1>
  <!-- R43/S4: one line saying what this screen is for. -->
  <p class="purpose">{settingsSectionPurpose("reading", t)}</p>

  <div class="panel">
    <h2>{t("provider.heading")}</h2>
    {#if providerError !== null}
      <p class="state error">{t("settings.providers.error", { message: providerError })}</p>
    {:else if providerMessage !== null}
      <p class="state">{providerMessage}</p>
    {:else}
      <ul>
        {#each providers ?? [] as provider (provider.id)}
          <li>
            <span>{describeProvider(provider, t)}</span>
            <!-- The stable provider id is shown as well as the readable sentence: it is what a
                 contributor implements and what stored progress rows reference, so it must be
                 visible rather than hidden behind a display name. -->
            <span class="detail id">{provider.id}</span>
          </li>
        {/each}
      </ul>
    {/if}
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
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    padding: 0.8rem 0.9rem;
  }

  .state {
    margin: 0;
    color: var(--fg-muted);
  }

  .state.error {
    color: var(--danger);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  li {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    font-size: var(--text-sm);
    color: var(--fg-muted);
  }

  .detail {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  /* The provider's stable id: metadata, not prose, so it is the quietest thing on the row — and it is
     set in the only mono face the platform can be relied on to have. */
  .id {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    color: var(--fg-subtle);
  }
</style>
