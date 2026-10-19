<script lang="ts">
  import { onMount } from "svelte";
  import { translate, type Locale } from "./lib/i18n";
  import {
    LIBRARY_CHANGED_EVENT,
    folderActionOutcome,
    startLibraryRefresh,
    type HomeCategory,
    type LibraryEntry,
    type OpenedFolder,
    type Provider,
    type ScanReport,
  } from "./lib/library";
  import {
    coveringFolder,
    hashForScreen,
    hashForView,
    isScreen,
    viewFromHash,
    type FolderRefusal,
    type FolderRemoval,
    type Screen,
    type View,
    type WatchedFolder,
  } from "./lib/screens";
  import { OPEN_SCREEN_EVENT, invoke, listen, runningInTauri, type Unlisten } from "./lib/tauri";
  import { loadLanguage, saveLanguage } from "./lib/settings";
  // L56. The suggestion queue's pure decisions live in their own module: what the section says in
  // each of its three states, what a decision does to the list, and what to do when one fails.
  import { suggestionOutcome, withoutSuggestion, type Suggestion } from "./lib/suggestions";
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

  // L56. `null` until the store answers, for the same reason as the providers and the folders: "still
  // checking" and "nothing is waiting for a decision" are different facts, and the section says which.
  let suggestions = $state<Suggestion[] | null>(null);
  let suggestionError = $state<string | null>(null);
  // True while a decision is being recorded, so neither button can be pressed twice — the backend
  // refuses a second decision on the same row, and the interface must not be the one to offer it.
  let deciding = $state(false);

  // R27 (L42). What removing a watched folder would take with it, while the confirmation is open. It is
  // the store's own plan and not a guess: the count the modal states comes from the same computation the
  // removal carries out, so the two cannot disagree.
  let removal = $state<FolderRemoval | null>(null);

  // R22. A folder inside a watched one is refused before anything is scanned, and the refusal is held
  // here rather than in the screen for the same reason the card's folder status is: the screen renders,
  // and the decisions live where the data does. Both of this screen's modals read from props, which is
  // also what lets a test see them without typing into an input.
  let refusal = $state<FolderRefusal | null>(null);

  let scanning = $state(false);
  let scanReport = $state<ScanReport | null>(null);
  let scanError = $state<string | null>(null);

  // R27. A folder removal takes that folder's documents with it, so the listing is momentarily empty
  // while the store is written. It is one of the explicit changes that is "in flight" for the screens.
  let removing = $state(false);

  // L47. True while a read the reader caused is running: the first load, a scan, or a folder removal.
  // The screens turn this into a loader through `libraryView` instead of their empty state, so an empty
  // listing never says "there is nothing" while the work is still in progress. The fifteen-second
  // re-read is deliberately NOT here: `refreshLibrary` raises no flag, so it stays silent over a listing
  // that already has content — which is why it is a separate path from the first load.
  const libraryInFlight = $derived(libraryLoading || scanning || removing);

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

  // R28 (#46). The listing is re-read while the application runs, because the facts it lists change on
  // their own: the tracker records a position every few seconds in the desktop process, so a document
  // gains its first record, moves through its pages, and reaches its last one without the reader touching
  // the interface. Reading it only at mount and after a scan made the screen look dead for as long as the
  // application was working — which is the defect this function exists to remove.
  //
  // It is deliberately NOT `loadLibrary`: that one raises the loading state, and "reading the library" may
  // only be said when there is nothing on screen yet. Raising it here would flash an empty grid over a
  // listing that is already there, once per interval.
  async function refreshLibrary(): Promise<void> {
    try {
      entries = await invoke<LibraryEntry[]>("list_library");
      libraryError = null;
    } catch (error) {
      // A failed re-read is reported where the first load's failure is reported, because it is the same
      // inability to read the library — and it clears on the next success, at most one interval away, so a
      // transient failure heals itself instead of leaving a listing that looks current and is not.
      libraryError = String(error);
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

  async function loadSuggestions(): Promise<void> {
    suggestionError = null;
    try {
      suggestions = await invoke<Suggestion[]>("list_suggestions");
    } catch (error) {
      suggestionError = String(error);
    }
  }

  /**
   * Records the reader's decision, and nothing decides for them.
   *
   * The order after a recorded decision matters: accepting has just put a document in the library, so
   * the listing is re-read **without** raising the loading state — the same silent path a change of
   * title uses — because flashing an empty grid over a listing that is already there would be worse
   * than the fraction of a second it saves. Either way the queue is re-read from the store, which is
   * the only thing that decides what is still pending.
   */
  /** Opens the folder that holds a suggested document, which is the only thing its path can do. */
  async function openSuggestionFolder(path: string): Promise<void> {
    const [settled] = await Promise.allSettled([
      invoke<OpenedFolder>("open_containing_folder", { path }),
    ]);
    const outcome = folderActionOutcome(settled);
    if (outcome.kind === "failed") {
      console.error("the folder could not be opened", outcome.message);
    }
  }

  async function decideSuggestion(id: number, accept: boolean): Promise<void> {
    suggestionError = null;
    deciding = true;
    const command = accept ? "accept_suggestion" : "discard_suggestion";
    const [settled] = await Promise.allSettled([invoke(command, { id })]);
    deciding = false;

    const outcome = suggestionOutcome(settled, id);
    if (outcome.kind === "failed") {
      suggestionError = outcome.message;
      return;
    }

    // The item leaves the list at once — the backend will refuse a second decision, so leaving it
    // visible would offer a promise the application cannot keep — and the store is asked again below.
    suggestions = withoutSuggestion(suggestions ?? [], outcome.id);
    if (accept) {
      await refreshLibrary();
    }
    await loadSuggestions();
  }

  async function scan(path: string): Promise<void> {
    // R22: a folder inside a watched one is watched already, and adding it again would scan the same
    // files twice. The scan is not attempted at all: the interface states the refusal in the reader's own
    // language, naming the folder that already covers the choice. The Rust side enforces the same rule,
    // because it is the truth, but its message can only be in English — two purposes, two statements of
    // the same rule.
    const covered = coveringFolder(folders ?? [], path);
    if (covered !== null) {
      refusal = { covered, candidate: path };
      return;
    }
    refusal = null;
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

  // The card's containing-folder action (L27, L30). Both sentences that used to render its outcome were
  // removed at the user's request, so there is no state to hold and no screen to update; what remains is
  // the decision the command's answer forces, which `folderActionOutcome` makes as a pure rule. A failure
  // is reported where the application reports failures that have no user state to carry them — the same
  // console the window actions use.
  async function openContainingFolder(entry: LibraryEntry): Promise<void> {
    const [settled] = await Promise.allSettled([
      invoke<OpenedFolder>("open_containing_folder", { path: entry.path }),
    ]);
    const outcome = folderActionOutcome(settled);
    if (outcome.kind === "failed") {
      console.error("the folder could not be opened", outcome.message);
    }
  }

  // R27: removing a watch now takes that folder's documents out of the library with it, so the click no
  // longer removes anything — it asks the store what the removal would do, and the modal says so. A plan
  // that comes back empty means the folder is already gone (another window, an edited database): there is
  // nothing to confirm, so the list is re-read and the answer is corrected rather than announced.
  async function askRemoval(id: number): Promise<void> {
    foldersError = null;
    try {
      const plan = await invoke<FolderRemoval | null>("folder_removal", { id });
      if (plan === null) {
        await loadFolders();
        return;
      }
      removal = plan;
    } catch (error) {
      foldersError = String(error);
    }
  }

  // The removal takes documents with it, so both views are refreshed: the folder list that named it and
  // the library it emptied. The modal is closed before the call rather than after, so a failure cannot
  // leave a stale confirmation on screen over a store that has already changed.
  async function confirmRemoval(): Promise<void> {
    const pending = removal;
    if (pending === null) {
      return;
    }
    removal = null;
    removing = true;
    try {
      await invoke("remove_folder", { id: pending.folder.id });
      await Promise.all([loadFolders(), loadLibrary()]);
    } catch (error) {
      foldersError = String(error);
    } finally {
      removing = false;
    }
  }

  // R24: opening a watched folder opens the folder *itself*, which is why it is not
  // `open_containing_folder` — that one opens the folder containing a document.
  async function openFolder(path: string): Promise<void> {
    try {
      await invoke("open_folder", { path });
    } catch (error) {
      // Nowhere on this screen to report it, and failing to open a folder is a program error rather
      // than user state, so it goes to the console instead of being discarded.
      console.error("the folder could not be opened", error);
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
    let stopLibraryRefresh: (() => void) | null = null;
    let cancelled = false;

    // R31: a card can name a document on any screen, including the category screen, which is not
    // given a screen-level callback. The change is announced instead, and the owner of the listing
    // re-reads it here, so a saved title appears everywhere at once rather than waiting for the
    // refresh interval. The listener is removed with the rest on unmount.
    const onLibraryChanged = (): void => {
      void refreshLibrary();
    };
    window.addEventListener(LIBRARY_CHANGED_EVENT, onLibraryChanged);

    if (inTauri) {
      void loadStoredLanguage();
      void loadLibrary();
      void loadProviders();
      void loadFolders();
      void loadSuggestions();
      // R28: the same interval keeps the listing current, and it starts with the application rather than
      // with a screen, because the listing is what the screens receive. Outside the shell there is no store
      // to re-read, so the clock does not start.
      stopLibraryRefresh = startLibraryRefresh(() => {
        void refreshLibrary();
        // The queue fills itself in the desktop process, so the same interval is what keeps the
        // section current — otherwise a document the reader opened in their viewer would sit unseen
        // until something else happened to re-read it.
        void loadSuggestions();
      });
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
      window.removeEventListener(LIBRARY_CHANGED_EVENT, onLibraryChanged);
      stopListening?.();
      stopLibraryRefresh?.();
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
        inFlight={libraryInFlight}
        error={libraryError}
        {t}
        onOpenFolder={openContainingFolder}
        onOpenCategory={selectCategory}
        onNavigate={selectScreen}
      />
    {:else if view.kind === "screen" && view.screen === "library"}
      <LibraryScreen
        {entries}
        inFlight={libraryInFlight}
        error={libraryError}
        locale={locale}
        {t}
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
        {suggestions}
        {suggestionError}
        {deciding}
        onOpenDocument={openSuggestionFolder}
        onDecide={decideSuggestion}
        onScan={scan}
        onLocale={chooseLocale}
        onOpenFolder={openFolder}
        {removal}
        onAskRemoval={askRemoval}
        onConfirmRemoval={confirmRemoval}
        onCancelRemoval={() => (removal = null)}
        {refusal}
        onCloseRefusal={() => (refusal = null)}
        {inTauri}
      />
    {:else}
      <CategoryScreen
        category={view.category}
        {entries}
        inFlight={libraryInFlight}
        error={libraryError}
        {t}
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
