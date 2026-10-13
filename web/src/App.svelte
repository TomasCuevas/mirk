<script lang="ts">
  import { onMount } from "svelte";
  import { translate, type Locale } from "./lib/i18n";
  import {
    describeEntryProgress,
    describeLibraryMessage,
    describeProvider,
    describeProviderRegistry,
    describeScanSummary,
    type LibraryEntry,
    type Provider,
    type ScanReport,
  } from "./lib/library";
  import { titleFromPath } from "./lib/library";
  import { invoke, runningInTauri } from "./lib/tauri";

  const inTauri = runningInTauri();

  let locale = $state<Locale>("en");
  const t = $derived(translate(locale));

  let entries = $state<LibraryEntry[]>([]);
  let libraryLoading = $state(true);
  let libraryError = $state<string | null>(null);
  const libraryMessage = $derived(describeLibraryMessage(entries, t));

  // `null` until the registry answers, so the screen cannot claim there are no providers before it
  // knows: "still checking" and "there are none" are different facts.
  let providers = $state<Provider[] | null>(null);
  let providerError = $state<string | null>(null);
  const providerMessage = $derived(describeProviderRegistry(providers, t));

  let folder = $state("");
  let scanning = $state(false);
  let scanReport = $state<ScanReport | null>(null);
  let scanError = $state<string | null>(null);

  async function loadLibrary(): Promise<void> {
    libraryLoading = true;
    libraryError = null;
    try {
      entries = await invoke<LibraryEntry[]>("list_library");
    } catch (error) {
      libraryError = String(error);
    } finally {
      libraryLoading = false;
    }
  }

  async function loadProviders(): Promise<void> {
    providerError = null;
    try {
      providers = await invoke<Provider[]>("progress_providers");
    } catch (error) {
      providerError = String(error);
    }
  }

  async function scan(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const path = folder.trim();
    if (path === "") {
      return;
    }

    scanning = true;
    scanError = null;
    scanReport = null;
    try {
      scanReport = await invoke<ScanReport>("scan_folder", { folder: path });
      await loadLibrary();
    } catch (error) {
      scanError = String(error);
    } finally {
      scanning = false;
    }
  }

  onMount(() => {
    if (!inTauri) {
      libraryLoading = false;
      return;
    }
    void loadLibrary();
    void loadProviders();
  });
</script>

<main>
  <header>
    <h1>{t("app.name")}</h1>
    <label class="language">
      <span>{t("language.label")}</span>
      <select bind:value={locale}>
        <option value="en">{t("language.en")}</option>
        <option value="es">{t("language.es")}</option>
      </select>
    </label>
  </header>

  {#if !inTauri}
    <p class="state">{t("app.outsideTauri")}</p>
  {/if}

  <section>
    <h2>{t("library.heading")}</h2>
    {#if libraryLoading}
      <p class="state">{t("library.loading")}</p>
    {:else if libraryError !== null}
      <p class="state error">{t("library.error", { message: libraryError })}</p>
    {:else if libraryMessage !== null}
      <p class="state">{libraryMessage}</p>
    {:else}
      <ul class="library">
        {#each entries as entry (entry.id)}
          <li>
            <span class="title">{entry.title ?? titleFromPath(entry.path)}</span>
            {#if entry.author !== null}
              <span class="author">{t("library.author", { author: entry.author })}</span>
            {/if}
            <span class="pages">
              {entry.pages === null
                ? t("library.pagesUnknown")
                : t("library.pages", { count: entry.pages })}
            </span>
            <span class="progress">{describeEntryProgress(entry.progress, t)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section>
    <h2>{t("scan.heading")}</h2>
    <form onsubmit={scan}>
      <label for="folder">{t("scan.folderLabel")}</label>
      <input id="folder" bind:value={folder} placeholder={t("scan.placeholder")} />
      <button type="submit" disabled={scanning}>{t("scan.action")}</button>
    </form>

    {#if scanning}
      <p class="state">{t("scan.running")}</p>
    {/if}

    {#if scanError !== null}
      <p class="state error">{t("scan.error", { message: scanError })}</p>
    {/if}

    {#if scanReport !== null}
      <p>{describeScanSummary(scanReport, t)}</p>
      {#if scanReport.failures.length > 0}
        <ul class="failures">
          {#each scanReport.failures as failure}
            <li>{t("scan.failure", { path: failure.path, reason: failure.reason })}</li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>

  <section>
    <h2>{t("provider.heading")}</h2>
    {#if providerError !== null}
      <p class="state error">{providerError}</p>
    {:else if providerMessage !== null}
      <p class="state">{providerMessage}</p>
    {:else}
      <ul class="providers">
        {#each providers ?? [] as provider (provider.id)}
          <li>{describeProvider(provider, t)}</li>
        {/each}
      </ul>
    {/if}
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    background: #f6f7f9;
    color: #1c2024;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    line-height: 1.5;
  }

  main {
    max-width: 52rem;
    margin: 0 auto;
    padding: 2rem 1.5rem 4rem;
    display: flex;
    flex-direction: column;
    gap: 1.75rem;
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 1rem;
  }

  h1 {
    margin: 0;
    font-size: 1.6rem;
    letter-spacing: 0.02em;
  }

  h2 {
    margin: 0 0 0.75rem;
    font-size: 1rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #5a6472;
  }

  .language {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    color: #5a6472;
  }

  section {
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
    padding: 1.25rem 1.5rem;
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
    gap: 0.75rem;
  }

  .library li {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 0.15rem 1rem;
    padding-bottom: 0.75rem;
    border-bottom: 1px solid #eef0f3;
  }

  .library li:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .author,
  .pages {
    font-size: 0.85rem;
    color: #5a6472;
  }

  .author {
    grid-column: 1;
  }

  .pages {
    grid-column: 2;
    grid-row: 1;
    white-space: nowrap;
  }

  .progress {
    grid-column: 1 / -1;
    color: #24506b;
  }

  form {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
  }

  form label {
    font-size: 0.85rem;
    color: #5a6472;
  }

  input {
    flex: 1 1 18rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    font: inherit;
  }

  button {
    padding: 0.5rem 1rem;
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

  .providers {
    gap: 0.35rem;
  }
</style>
