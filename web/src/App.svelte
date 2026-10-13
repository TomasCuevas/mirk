<script lang="ts">
  import { onMount } from "svelte";
  import { translate, type Locale } from "./lib/i18n";
  import {
    type FolderActionStatus,
    type LibraryEntry,
    type OpenedFolder,
    type Provider,
    type ScanReport,
  } from "./lib/library";
  import {
    hashForScreen,
    screenFromHash,
    type Screen,
    type WatchedFolder,
  } from "./lib/screens";
  import { invoke, runningInTauri } from "./lib/tauri";
  import HomeScreen from "./components/HomeScreen.svelte";
  import LibraryScreen from "./components/LibraryScreen.svelte";
  import Navbar from "./components/Navbar.svelte";
  import SettingsScreen from "./components/SettingsScreen.svelte";

  const inTauri = runningInTauri();

  let locale = $state<Locale>("en");
  const t = $derived(translate(locale));

  // Navigation is a hash so a reload keeps the reader on the screen they were looking at. The
  // initial value is Inicio; `onMount` reads the real hash and a `hashchange` listener keeps
  // back/forward working.
  let screen = $state<Screen>("home");

  let entries = $state<LibraryEntry[]>([]);
  let libraryLoading = $state(true);
  let libraryError = $state<string | null>(null);

  // `null` until the registry answers, so the screen cannot claim there are no providers before it
  // knows: "still checking" and "there are none" are different facts.
  let providers = $state<Provider[] | null>(null);
  let providerError = $state<string | null>(null);

  // `null` until the store answers, for the same reason as the providers.
  let folders = $state<WatchedFolder[] | null>(null);
  let foldersError = $state<string | null>(null);

  // The result of the last card action. Held here, not in the card, so the card stays a renderer.
  let folderStatus = $state<FolderActionStatus | null>(null);

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

  async function loadFolders(): Promise<void> {
    foldersError = null;
    try {
      folders = await invoke<WatchedFolder[]>("watched_folders");
    } catch (error) {
      foldersError = String(error);
    }
  }

  async function scan(path: string): Promise<void> {
    scanning = true;
    scanError = null;
    scanReport = null;
    try {
      scanReport = await invoke<ScanReport>("scan_folder", { folder: path });
      // A scan can add documents and record the folder it walked, so both views are refreshed.
      await Promise.all([loadLibrary(), loadFolders()]);
    } catch (error) {
      scanError = String(error);
    } finally {
      scanning = false;
    }
  }

  async function openContainingFolder(entry: LibraryEntry): Promise<void> {
    folderStatus = null;
    try {
      const opened = await invoke<OpenedFolder>("open_containing_folder", { path: entry.path });
      folderStatus = { id: entry.id, kind: "ok", folder: opened.folder, viewer: opened.viewer };
    } catch (error) {
      folderStatus = { id: entry.id, kind: "error", message: String(error) };
    }
  }

  function selectScreen(next: Screen): void {
    screen = next;
    window.location.hash = hashForScreen(next);
  }

  onMount(() => {
    screen = screenFromHash(window.location.hash);
    const sync = (): void => {
      screen = screenFromHash(window.location.hash);
    };
    window.addEventListener("hashchange", sync);

    if (inTauri) {
      void loadLibrary();
      void loadProviders();
      void loadFolders();
    } else {
      // Outside the shell there is nothing to ask, and saying the library is empty would be a lie.
      libraryLoading = false;
    }

    return () => window.removeEventListener("hashchange", sync);
  });
</script>

<div class="app">
  <header>
    <span class="brand">{t("app.name")}</span>
  </header>

  {#if !inTauri}
    <p class="state">{t("app.outsideTauri")}</p>
  {/if}

  <main>
    {#if screen === "home"}
      <HomeScreen {t} />
    {:else if screen === "library"}
      <LibraryScreen
        {entries}
        loading={libraryLoading}
        error={libraryError}
        locale={locale}
        {t}
        {folderStatus}
        onOpenFolder={openContainingFolder}
      />
    {:else}
      <SettingsScreen
        {providers}
        {providerError}
        {folders}
        {foldersError}
        {locale}
        {t}
        {scanning}
        {scanReport}
        {scanError}
        onScan={scan}
        onLocale={(next) => (locale = next)}
      />
    {/if}
  </main>

  <Navbar {screen} {t} onSelect={selectScreen} />
</div>

<style>
  :global(body) {
    margin: 0;
    background: #f6f7f9;
    color: #1c2024;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    line-height: 1.5;
  }

  .app {
    max-width: 68rem;
    margin: 0 auto;
    padding: 1.5rem 1.5rem 6rem;
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
  }

  header {
    display: flex;
    align-items: baseline;
  }

  .brand {
    font-size: 1.1rem;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: lowercase;
  }

  .state {
    margin: 0;
    color: #5a6472;
  }

  main {
    display: flex;
    flex-direction: column;
  }
</style>
