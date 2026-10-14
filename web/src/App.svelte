<script lang="ts">
  import { onMount } from "svelte";
  import { translate, type Locale } from "./lib/i18n";
  import {
    type FolderActionStatus,
    type HomeCategory,
    type LibraryEntry,
    type OpenedFolder,
    type Provider,
    type ScanReport,
  } from "./lib/library";
  import {
    hashForScreen,
    hashForView,
    isScreen,
    viewFromHash,
    type Screen,
    type View,
    type WatchedFolder,
  } from "./lib/screens";
  import { OPEN_SCREEN_EVENT, invoke, listen, runningInTauri, type Unlisten } from "./lib/tauri";
  import { loadLanguage, saveLanguage } from "./lib/settings";
  import { type WindowControl } from "./lib/window";
  import CategoryScreen from "./components/CategoryScreen.svelte";
  import HomeScreen from "./components/HomeScreen.svelte";
  import LibraryScreen from "./components/LibraryScreen.svelte";
  import Navbar from "./components/Navbar.svelte";
  import SettingsScreen from "./components/SettingsScreen.svelte";
  import WindowBar from "./components/WindowBar.svelte";

  const inTauri = runningInTauri();

  let locale = $state<Locale>("en");
  const t = $derived(translate(locale));

  // Navigation is a hash so a reload keeps the reader on the screen they were looking at. The
  // initial value is Inicio; `onMount` reads the real hash and a `hashchange` listener keeps
  // back/forward working. A view is either a navbar screen or one home category's own list (B2).
  let view = $state<View>({ kind: "screen", screen: "home" });

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

  // R1 (L22). The language is not the interface's private state: the desktop process builds the
  // tray's words from the same stored value, so it is read when the interface starts and written
  // when the user chooses one. A read that fails leaves the default standing and is reported to the
  // console; a write that fails must never take the switch away from the user, so the screen changes
  // language either way and the failure is reported rather than discarded.
  async function loadStoredLanguage(): Promise<void> {
    try {
      const stored = await loadLanguage(invoke);
      if (stored) {
        locale = stored;
      }
    } catch (error) {
      console.error("the stored language could not be read", error);
    }
  }

  async function chooseLocale(next: Locale): Promise<void> {
    locale = next;
    if (!inTauri) {
      return;
    }
    try {
      await saveLanguage(invoke, next);
    } catch (error) {
      console.error("the language could not be stored", error);
    }
  }

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
    view = { kind: "screen", screen: next };
    window.location.hash = hashForScreen(next);
  }

  // B2: a carousel's button opens that category's full list on a screen of its own.
  function selectCategory(category: HomeCategory): void {
    const next: View = { kind: "category", category };
    view = next;
    window.location.hash = hashForView(next);
  }

  // Window chrome actions are fire-and-forget: the top bar has nowhere to render an error, and a
  // failed window operation is a program error rather than user state. It goes to the console
  // instead of being silently discarded.
  async function runWindowAction(control: WindowControl): Promise<void> {
    if (!inTauri) {
      return;
    }
    try {
      await invoke(control.command);
    } catch (error) {
      console.error(`window action ${control.action} failed`, error);
    }
  }

  // Pressing the top bar starts moving the undecorated window. This is an application command, not
  // Tauri's drag-region attribute, so a double-click has nowhere to go (A2).
  async function startDrag(): Promise<void> {
    if (!inTauri) {
      return;
    }
    try {
      await invoke("start_drag");
    } catch (error) {
      console.error("window drag failed", error);
    }
  }

  onMount(() => {
    view = viewFromHash(window.location.hash);
    const sync = (): void => {
      view = viewFromHash(window.location.hash);
    };
    window.addEventListener("hashchange", sync);

    // R12: the tray's settings entry is native and cannot reach a screen by itself, so the desktop
    // process asks the interface to navigate. The request names a screen and the interface answers by
    // writing the same hash the navbar writes, so a tray click and a navbar click land on the same
    // screen through the same path — the hash listener above is what actually moves. A request that
    // names no screen is ignored rather than guessed at: it comes from outside the interface.
    let stopListening: Unlisten | null = null;
    let cancelled = false;

    if (inTauri) {
      void loadStoredLanguage();
      void loadLibrary();
      void loadProviders();
      void loadFolders();
      void listen<string>(OPEN_SCREEN_EVENT, (screen) => {
        if (isScreen(screen)) {
          window.location.hash = hashForScreen(screen);
        }
      })
        .then((stop) => {
          // The listener may arrive after the screen is already gone; stopping it then is still owed.
          if (cancelled) {
            stop();
          } else {
            stopListening = stop;
          }
        })
        .catch((error) => {
          // A listener that never arrives is a real failure, not a detail: the tray's settings entry
          // would then do nothing at all. This went unnoticed until the user clicked that entry,
          // because an unhandled rejection is silent — the permission the core event API needs was
          // missing, and nothing anywhere said so. Reported, never dropped.
          console.error("the tray's screen requests will not be received", error);
        });
    } else {
      // Outside the shell there is nothing to ask, and saying the library is empty would be a lie.
      libraryLoading = false;
    }

    return () => {
      cancelled = true;
      window.removeEventListener("hashchange", sync);
      stopListening?.();
    };
  });
</script>

<div class="app">
  <WindowBar {t} {inTauri} onAction={runWindowAction} onDrag={startDrag} />

  <main>
    {#if !inTauri}
      <p class="state">{t("app.outsideTauri")}</p>
    {/if}

    {#if view.kind === "screen" && view.screen === "home"}
      <HomeScreen
        {entries}
        loading={libraryLoading}
        error={libraryError}
        {t}
        {folderStatus}
        onOpenFolder={openContainingFolder}
        onOpenCategory={selectCategory}
      />
    {:else if view.kind === "screen" && view.screen === "library"}
      <LibraryScreen
        {entries}
        loading={libraryLoading}
        error={libraryError}
        locale={locale}
        {t}
        {folderStatus}
        onOpenFolder={openContainingFolder}
      />
    {:else if view.kind === "screen"}
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
        onLocale={chooseLocale}
      />
    {:else}
      <CategoryScreen
        category={view.category}
        {entries}
        loading={libraryLoading}
        error={libraryError}
        {t}
        {folderStatus}
        onOpenFolder={openContainingFolder}
        onBack={() => selectScreen("home")}
      />
    {/if}
  </main>

  <!-- A category list belongs to Inicio, so the navbar keeps Inicio marked while the reader is on
       one, rather than marking nothing. -->
  <Navbar screen={view.kind === "screen" ? view.screen : "home"} {t} onSelect={selectScreen} />
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
  }

  :global(body) {
    background: #f6f7f9;
    color: #1c2024;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    line-height: 1.5;
  }

  :global(#app) {
    height: 100%;
  }

  /* The window is a fixed phone shape, so the layout fills it exactly: the top bar and the navbar
     stay put and only the screen between them scrolls. */
  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .state {
    margin: 0 0 0.75rem;
    color: #5a6472;
  }

  main {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    /* R6 (L22): the layout padding was too generous for a phone-width window, so it is reduced here
       and in the screens and cards together rather than one place at a time. The top value is the
       exception and went up, not down: the user found each screen's own heading too close to the
       window bar (L23), and this is the one place that spacing can be stated for every screen. */
    padding: 1.1rem 0.8rem 0.9rem;
  }
</style>
