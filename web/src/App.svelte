<script lang="ts">
  import { onMount } from "svelte";
  import { translate, type Locale } from "./lib/i18n";
  import {
    LIBRARY_CHANGED_EVENT,
    documentActionOutcome,
    folderActionOutcome,
    startLibraryRefresh,
    type HomeCategory,
    type LibraryEntry,
    type OpenedDocument,
    type OpenedFolder,
    type Provider,
    type ScanReport,
  } from "./lib/library";
  import {
    coveringFolder,
    hashForScreen,
    hashForView,
    navbarScreen,
    isScreen,
    viewFromHash,
    type FolderRefusal,
    type FolderRemoval,
    type Screen,
    type SettingsSection,
    type View,
    type WatchedFolder,
  } from "./lib/screens";
  import { OPEN_SCREEN_EVENT, invoke, listen, runningInTauri, type Unlisten } from "./lib/tauri";
  import { loadLanguage, loadWindowLayout, saveLanguage, saveWindowLayout } from "./lib/settings";
  // L56. The suggestion queue's pure decisions live in their own module: what the section says in
  // each of its three states, what a decision does to the list, and what to do when one fails.
  import {
    detectorAfterFailure,
    detectorBelief,
    detectorSettled,
    suggestionOutcome,
    withoutSuggestion,
    type DetectorBelief,
    type Suggestion,
  } from "./lib/suggestions";
  import {
    DEFAULT_WINDOW_LAYOUT,
    gridCardMinimum,
    gridColumns,
    interfaceFontSize,
    isWindowLayout,
    loadWindowPreset,
    navMode,
    RESTORE_WINDOW_LAYOUT,
    WINDOW_LAYOUT_CHANGED_EVENT,
    type WindowControl,
    type WindowLayout,
  } from "./lib/window";
  import CategoryScreen from "./components/CategoryScreen.svelte";
  import HomeScreen from "./components/HomeScreen.svelte";
  import LibraryScreen from "./components/LibraryScreen.svelte";
  import Navbar from "./components/Navbar.svelte";
  import SettingsScreen from "./components/SettingsScreen.svelte";
  import SettingsAppScreen from "./components/SettingsAppScreen.svelte";
  import SettingsLibraryScreen from "./components/SettingsLibraryScreen.svelte";
  import SettingsReadingScreen from "./components/SettingsReadingScreen.svelte";
  import SettingsWindowScreen from "./components/SettingsWindowScreen.svelte";
  import WindowBar from "./components/WindowBar.svelte";

  const inTauri = runningInTauri();

  let locale = $state<Locale>("en");
  const t = $derived(translate(locale));

  // S2–S4 of `odd/tasks/window-layout.md`. The layout is the reader's stored choice, and the shell is
  // where it becomes the grid's column count: every screen hands its entries to the same grid, so the
  // value is published here once instead of being threaded through each screen. The desktop process
  // sizes the window from the same stored value, which is what keeps "three per row" from meaning a
  // narrow window with three squeezed cards.
  let windowLayout = $state<WindowLayout>(DEFAULT_WINDOW_LAYOUT);
  // S3 of `odd/tasks/navigation.md`: full screen has the room for a left sidebar, so the navigation
  // changes place with the layout. A windowed layout — however wide — keeps the bottom bar.
  const sidebar = $derived(navMode(windowLayout) === "side");
  // R7 of `odd/tasks/desktop-chrome.md`: full screen is the layout a store that has never chosen
  // opens with, so "restore" cannot fall back to the default — it returns to three per row, which
  // the user named. A reader who chose a preset still gets that preset back while the application
  // runs, because the choice is remembered below.
  let lastPreset = $state<WindowLayout>(RESTORE_WINDOW_LAYOUT);
  const maximized = $derived(windowLayout === "auto");

  // Navigation is a hash so a reload keeps the reader on the screen they were looking at. The
  // initial value is Inicio; `onMount` reads the real hash and a `hashchange` listener keeps
  // back/forward working. A view is either a navbar screen or one home category's own list (B2).
  let view = $state<View>({ kind: "screen", screen: "home" });

  // The one scrolling container of the shell (only the screen between the top bar and the navbar
  // scrolls). Bound so a navigation can put the reader back at the top of the screen they just chose.
  let scroller = $state<HTMLElement | null>(null);

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
  /** Four beliefs, not a flag: only "unsupported" is a permanent fact about the machine (L63). */
  let detector = $state<DetectorBelief>("unknown");
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

  // S3: the stored layout is read at startup so the grid matches the window the desktop process already
  // sized. A failed read is reported and the default stands, the same way a failed language read is.
  async function loadStoredWindowLayout(): Promise<void> {
    try {
      const stored = await loadWindowLayout(invoke);
      if (stored) {
        windowLayout = stored;
        if (stored !== "auto") {
          lastPreset = stored;
        }
      }
    } catch (error) {
      console.error("the stored window layout could not be read", error);
    }
  }

  // S3: the layout says what the window is now; the preset says which rows it comes back to when it
  // leaves full screen. The process resolves its own fallback, so a non-null answer is the value its own
  // drag would use — the two halves must not answer that question differently. A failed read is reported
  // and the fallback stands, the same way a failed layout read is.
  async function loadStoredWindowPreset(): Promise<void> {
    try {
      const remembered = await loadWindowPreset(invoke);
      if (remembered) {
        lastPreset = remembered;
      }
    } catch (error) {
      console.error("the stored window preset could not be read", error);
    }
  }

  // Choosing a layout is one command: the desktop process stores it and sizes its window from it. The
  // grid changes at once, because the reader pressed a choice and must see it; a failure then re-reads
  // the store rather than leaving the grid claiming a layout the window never took.
  async function chooseWindowLayout(next: WindowLayout): Promise<void> {
    if (next !== "auto") {
      lastPreset = next;
    }
    windowLayout = next;
    if (!inTauri) {
      return;
    }
    try {
      await saveWindowLayout(invoke, next);
    } catch (error) {
      console.error("the window layout could not be applied", error);
      await loadStoredWindowLayout();
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

  /** Asks until the answer is definitive; asking once turned a failure into a permanent claim (L63). */
  async function loadSuggestionCapability(): Promise<void> {
    if (detectorSettled(detector)) {
      return;
    }
    try {
      const view = await invoke<{ id: string; availability: string }>(
        "suggestion_detector",
      );
      detector = detectorBelief(view.availability);
    } catch {
      detector = detectorAfterFailure(detector);
    }
  }

  async function loadSuggestions(): Promise<void> {
    void loadSuggestionCapability();
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

  // The card's open action (R37): the document goes to the system's own application for it, at the
  // page the reader last reached. The card renders no outcome — the folder action's two sentences
  // were removed at the user's request (L27, L30) and this one is the same kind of action — so a
  // failure is reported to the console, which is where the window actions report the failures that
  // have no user state to carry them.
  async function openDocument(entry: LibraryEntry): Promise<void> {
    const [settled] = await Promise.allSettled([
      invoke<OpenedDocument>("open_document", { id: entry.id }),
    ]);
    const outcome = documentActionOutcome(settled);
    if (outcome.kind === "failed") {
      console.error("the document could not be opened", outcome.message);
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

  // R43/S2: Ajustes' rows open a sub-screen that is a view of its own, so a reload lands there and
  // Back returns to the index, exactly as a category does against Inicio.
  function openSettingsSection(section: SettingsSection): void {
    const next: View = { kind: "settings", section };
    view = next;
    window.location.hash = hashForView(next);
  }

  // S4: the double click on the top bar and the maximise control are one gesture with two triggers, so
  // both call this. It switches the stored layout rather than a geometry of its own, which is what keeps
  // the gesture, the button and the option selected in Ajustes → Ventana from disagreeing.
  async function toggleWindowLayout(): Promise<void> {
    await chooseWindowLayout(maximized ? lastPreset : "auto");
  }

  // Window chrome actions are fire-and-forget: the top bar has nowhere to render an error, and a
  // failed window operation is a program error rather than user state. It goes to the console
  // instead of being silently discarded.
  async function runWindowAction(control: WindowControl): Promise<void> {
    if (!inTauri) {
      return;
    }
    // S4: the maximise control is not a window command of its own — it chooses the same stored layout
    // Ajustes → Ventana does, so the button, the window size and the selected option cannot disagree.
    if (control.action === "maximize") {
      await toggleWindowLayout();
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

  // A new screen starts at its own top, never at the scroll position the previous one left behind.
  //
  // The user's report, and it is the whole rule: *"si se ha hecho scroll en una pantalla y luego se
  // navega a otra, esta nueva pantalla no resetea el scroll, permanece con el scroll ya desplazado y
  // esto es muy molesto"*. The shell has ONE scrolling container — the screen between the top bar and
  // the navbar — so its `scrollTop` outlives the screen that filled it unless something resets it.
  //
  // It lives on `view` rather than on the three navigation functions because `view` is the single place
  // every way of moving ends up: the navbar, a card that opens a category, a settings row, the tray's
  // own request, and the hash listener that answers Back and Forward. An effect runs AFTER the DOM has
  // been updated with the new screen, so the reset lands on the new content rather than on the old one.
  function resetScroll(): void {
    if (scroller !== null) {
      scroller.scrollTop = 0;
    }
  }

  // L15: full screen is read from farther away, so the interface is drawn on a larger root. It is one
  // value and not a dozen per-component overrides: the token layer is written in `rem`, so this scales
  // the type, the spacing, the control heights and the sidebar together and they cannot drift apart.
  $effect(() => {
    const size = `${interfaceFontSize(windowLayout)}px`;
    if (typeof document === "undefined") {
      return;
    }
    const root = document.documentElement;
    root.style.fontSize = size;
    return () => {
      root.style.fontSize = "";
    };
  });

  $effect(() => {
    // Reading `view` is what subscribes this effect to navigation; every navigation replaces it.
    void view;
    resetScroll();
  });

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
    let stopLayoutChanges: Unlisten | null = null;
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
      void loadStoredWindowLayout();
      void loadStoredWindowPreset();
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
      // S2/T3: the window's own gesture can change the stored layout behind the interface — dragging it
      // out of full screen unmaximises it, and the desktop process rewrites the layout to the reader's
      // preset. It announces the key and the store is re-read here rather than the payload being
      // trusted: what the store says is the one fact the window, Ajustes and the grid can agree on.
      void listen<string>(WINDOW_LAYOUT_CHANGED_EVENT, (layout) => {
        // A key this build does not know is ignored rather than guessed at: it comes from outside the
        // interface, and full screen is not a preset to come back to.
        if (isWindowLayout(layout) && layout !== "auto") {
          lastPreset = layout;
        }
        void loadStoredWindowLayout();
      })
        .then((stop) => {
          if (cancelled) {
            stop();
          } else {
            stopLayoutChanges = stop;
          }
        })
        .catch((error) => {
          // Same failure as the tray's requests, and the same consequence: without this listener the
          // interface keeps drawing the layout the window has already left.
          console.error("the window layout changes will not be received", error);
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
      stopLayoutChanges?.();
      stopLibraryRefresh?.();
    };
  });
</script>

<div
    class="app"
    style:--grid-columns={gridColumns(windowLayout)}
    style:--grid-card-min={`${gridCardMinimum(windowLayout)}px`}
  >
  <!-- L4: the light the glass is made of. Two soft blooms and a whisper of sheen, behind everything and
       never interactive; the panes above blur it, so the interface reads as layered rather than painted. -->
  <div class="aurora" aria-hidden="true"></div>

  <WindowBar
    {t}
    {inTauri}
    {maximized}
    onAction={runWindowAction}
    onDrag={startDrag}
    onToggle={toggleWindowLayout}
  />

  <!-- The shell's body is what the layout arranges: a column (screen, then bottom bar) in a window, a
       row (sidebar, then screen) in full screen. It is the same screen element either way, so the
       listing keeps its state and its scroll container while the navigation changes place. -->
  <div class="body" class:side={sidebar}>
    {#if sidebar}
      <Navbar mode="side" screen={navbarScreen(view)} {t} onSelect={selectScreen} />
    {/if}

    <main bind:this={scroller}>
    {#if !inTauri}
      <p class="state">{t("app.outsideTauri")}</p>
    {/if}

    {#if view.kind === "screen" && view.screen === "home"}
      <HomeScreen
        {entries}
        inFlight={libraryInFlight}
        error={libraryError}
        {locale}
        {t}
        onOpenFolder={openContainingFolder}
        onOpenDocument={openDocument}
        onOpenCategory={selectCategory}
        onNavigate={selectScreen}
      />
    {:else if view.kind === "screen" && view.screen === "library"}
      <LibraryScreen
        {entries}
        inFlight={libraryInFlight}
        error={libraryError}
        locale={locale}
        layout={windowLayout}
        {t}
        onOpenFolder={openContainingFolder}
        onOpenDocument={openDocument}
        onResetScroll={resetScroll}
      />
    {:else if view.kind === "settings" && view.section === "library"}
      <SettingsLibraryScreen
        {folders}
        {foldersError}
        {suggestions}
        {detector}
        {suggestionError}
        {deciding}
        {t}
        {scanning}
        {scanReport}
        {scanError}
        onScan={scan}
        onOpenFolder={openFolder}
        onOpenDocument={openSuggestionFolder}
        onDecide={decideSuggestion}
        {removal}
        onAskRemoval={askRemoval}
        onConfirmRemoval={confirmRemoval}
        onCancelRemoval={() => (removal = null)}
        {refusal}
        onCloseRefusal={() => (refusal = null)}
        {inTauri}
        onBack={() => selectScreen("settings")}
      />
    {:else if view.kind === "settings" && view.section === "reading"}
      <SettingsReadingScreen {providers} {providerError} {t} onBack={() => selectScreen("settings")} />
    {:else if view.kind === "settings" && view.section === "window"}
      <SettingsWindowScreen
        layout={windowLayout}
        {t}
        onLayout={chooseWindowLayout}
        onBack={() => selectScreen("settings")}
      />
    {:else if view.kind === "settings"}
      <SettingsAppScreen {locale} {t} onLocale={chooseLocale} onBack={() => selectScreen("settings")} />
    {:else if view.kind === "screen" && view.screen === "settings"}
      <SettingsScreen {t} onOpenSection={openSettingsSection} />
    {:else if view.kind === "category"}
      <CategoryScreen
        category={view.category}
        {entries}
        inFlight={libraryInFlight}
        error={libraryError}
        {locale}
        {t}
        onOpenFolder={openContainingFolder}
        onOpenDocument={openDocument}
        onBack={() => selectScreen("home")}
      />
    {/if}
  </main>

  <!-- A category list belongs to Inicio and a settings sub-screen belongs to Ajustes, so the navbar
       keeps the parent screen marked while the reader is inside one, rather than marking nothing. -->
    {#if !sidebar}
      <Navbar mode="bottom" screen={navbarScreen(view)} {t} onSelect={selectScreen} />
    {/if}
  </div>
</div>

<style>
  /* The window is a phone shape chosen from Ajustes → Ventana (S2), and the layout fills it exactly:
     the top bar and the navbar stay put and only the screen between them scrolls. `--grid-columns` is
     the layout published to the document grid; the canvas, the font and the base type live in
     `styles/tokens.css`, which is the single place a theme is decided. */
  .app {
    position: relative;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--canvas);
  }

  .aurora {
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    background:
      radial-gradient(48rem 32rem at 88% -8%, rgba(230, 163, 201, 0.14), transparent 62%),
      radial-gradient(44rem 30rem at -12% 18%, rgba(140, 126, 224, 0.16), transparent 65%),
      radial-gradient(38rem 26rem at 46% 112%, rgba(126, 112, 200, 0.12), transparent 62%),
      radial-gradient(30rem 14rem at 50% -4%, rgba(255, 255, 255, 0.035), transparent 70%),
      linear-gradient(180deg, rgba(255, 255, 255, 0.022), transparent 28%);
  }

  /* The aurora is positioned, so the shell's own layers are lifted above it explicitly. */
  .app > :global(header),
  .app > .body {
    position: relative;
    z-index: 1;
  }

  /* The body owns the room under the top bar. A window stacks the screen above the bottom bar, which is
     why the bar needs no fixed positioning (S2); full screen turns the same box into a row, with the
     sidebar taking its own width from the screen (S3). */
  .body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .body.side {
    flex-direction: row;
  }

  .state {
    margin: 0 0 var(--space-3);
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  main {
    flex: 1 1 auto;
    min-height: 0;
    /* A row's second child must be allowed to be narrower than its content, or a wide listing would
       push the sidebar off the window instead of scrolling inside itself. */
    min-width: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    /* R6 (L22): the layout padding was too generous for a phone-width window, so it is reduced here
       and in the screens and cards together rather than one place at a time. The top value is the
       exception and went up, not down: the user found each screen's own heading too close to the
       window bar (L23), and this is the one place that spacing can be stated for every screen. */
    padding: var(--space-4) var(--space-4) var(--space-4);
  }

  /* The scroll container of a dark canvas: a hairline under the top bar is the only edge it needs,
     and the overscroll bounce must not paint a light strip over the canvas. */
  main::-webkit-scrollbar {
    width: 0;
  }
</style>
