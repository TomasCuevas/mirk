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
  <button type="button" class="back" onclick={onBack}>{t("settings.back")}</button>

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
    font-size: 1.35rem;
  }

  .back {
    align-self: flex-start;
    padding: 0.35rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .back:hover {
    background: #f0f2f5;
  }

  .purpose {
    margin: 0;
    font-size: 0.85rem;
    color: #5a6472;
  }

  h2 {
    margin: 0 0 0.75rem;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #5a6472;
  }

  .panel {
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
    padding: 0.7rem 0.9rem;
  }

  .state {
    margin: 0;
    color: #5a6472;
  }

  .state.error {
    color: #a1301f;
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
  }

  .detail {
    font-size: 0.8rem;
    color: #5a6472;
  }

  .id {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    color: #8b95a3;
  }
</style>
