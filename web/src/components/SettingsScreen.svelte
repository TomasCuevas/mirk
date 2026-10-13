<script lang="ts">
  import type { Locale, Translate } from "../lib/i18n";
  import {
    describeProvider,
    describeProviderRegistry,
    describeScanSummary,
    type Provider,
    type ScanReport,
  } from "../lib/library";
  import { describeWatchedFolders, type WatchedFolder } from "../lib/screens";

  let {
    providers,
    providerError,
    folders,
    foldersError,
    locale,
    t,
    scanning,
    scanReport,
    scanError,
    onScan,
    onLocale,
  }: {
    providers: Provider[] | null;
    providerError: string | null;
    folders: WatchedFolder[] | null;
    foldersError: string | null;
    locale: Locale;
    t: Translate;
    scanning: boolean;
    scanReport: ScanReport | null;
    scanError: string | null;
    onScan: (folder: string) => Promise<void>;
    onLocale: (locale: Locale) => void;
  } = $props();

  const providerMessage = $derived(describeProviderRegistry(providers, t));
  const folderMessage = $derived(describeWatchedFolders(folders, t));

  let folder = $state("");

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const path = folder.trim();
    if (path === "") {
      return;
    }
    await onScan(path);
  }
</script>

<!--
  Configuración shows what genuinely exists (S5): the providers this build registers and whether
  this machine can use them, the language, and the folders the store has recorded. There are no
  invented settings — each section either has real state or says that it is empty.
-->
<section class="screen">
  <h1>{t("settings.heading")}</h1>

  <div class="panel">
    <h2>{t("settings.language.heading")}</h2>
    <label class="inline">
      <span>{t("language.label")}</span>
      <select
        value={locale}
        onchange={(event) => onLocale(event.currentTarget.value as Locale)}
      >
        <option value="en">{t("language.en")}</option>
        <option value="es">{t("language.es")}</option>
      </select>
    </label>
  </div>

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

  <div class="panel">
    <h2>{t("settings.folders.heading")}</h2>
    {#if foldersError !== null}
      <p class="state error">{t("settings.folders.error", { message: foldersError })}</p>
    {:else if folderMessage !== null}
      <p class="state">{folderMessage}</p>
    {:else}
      <ul>
        {#each folders ?? [] as watched (watched.id)}
          <li>
            <span class="path">{watched.path}</span>
            <span class="detail">
              {watched.recursive
                ? t("settings.folders.recursive")
                : t("settings.folders.notRecursive")},
              {watched.enabled
                ? t("settings.folders.enabled")
                : t("settings.folders.disabled")}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="panel">
    <h2>{t("scan.heading")}</h2>
    <form onsubmit={submit}>
      <label class="inline">
        <span>{t("scan.folderLabel")}</span>
        <input bind:value={folder} placeholder={t("scan.placeholder")} />
      </label>
      <button type="submit" disabled={scanning}>{t("scan.action")}</button>
    </form>

    {#if scanning}
      <p class="state">{t("scan.running")}</p>
    {/if}

    {#if scanError !== null}
      <p class="state error">{t("scan.error", { message: scanError })}</p>
    {/if}

    {#if scanReport !== null}
      <p class="state">{describeScanSummary(scanReport, t)}</p>
      {#if scanReport.failures.length > 0}
        <ul class="failures">
          {#each scanReport.failures as failure}
            <li>{t("scan.failure", { path: failure.path, reason: failure.reason })}</li>
          {/each}
        </ul>
      {/if}
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

  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.85rem;
    color: #5a6472;
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

  .path {
    overflow-wrap: anywhere;
  }

  .detail {
    font-size: 0.8rem;
    color: #5a6472;
  }

  .id {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    color: #8b95a3;
  }

  select,
  input {
    padding: 0.45rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    font: inherit;
    font-size: 0.9rem;
    color: #1c2024;
    box-sizing: border-box;
    max-width: 100%;
  }

  form {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 0.75rem;
  }

  form input {
    flex: 1 1 10rem;
    min-width: 0;
  }

  button {
    padding: 0.45rem 0.8rem;
    border: 1px solid #24506b;
    border-radius: 0.4rem;
    background: #24506b;
    color: #ffffff;
    font: inherit;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .failures {
    margin-top: 0.75rem;
    gap: 0.35rem;
    font-size: 0.85rem;
    color: #a1301f;
  }

  .failures li {
    overflow-wrap: anywhere;
  }
</style>
