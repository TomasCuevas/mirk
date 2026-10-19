<script lang="ts">
  import type { Locale, Translate } from "../lib/i18n";
  import {
    describeProvider,
    describeProviderRegistry,
    describeScanSummary,
    type Provider,
    type ScanReport,
  } from "../lib/library";
  import {
    describeCoveredFolder,
    describeRemoval,
    describeWatchedFolders,
    type FolderRefusal,
    type FolderRemoval,
    type WatchedFolder,
  } from "../lib/screens";
  import SuggestionCover from "./SuggestionCover.svelte";
  import {
    describeSuggestions,
    suggestionTitle,
    type Suggestion,
  } from "../lib/suggestions";
  import { invoke } from "../lib/tauri";
  import Modal from "./Modal.svelte";

  let {
    providers,
    providerError,
    folders,
    foldersError,
    suggestions,
    detectorOk,
    suggestionError,
    deciding,
    locale,
    t,
    scanning,
    scanReport,
    scanError,
    onScan,
    onLocale,
    onOpenFolder,
    onOpenDocument,
    onDecide,
    removal,
    onAskRemoval,
    onConfirmRemoval,
    onCancelRemoval,
    refusal,
    onCloseRefusal,
    inTauri,
  }: {
    providers: Provider[] | null;
    providerError: string | null;
    folders: WatchedFolder[] | null;
    foldersError: string | null;
    suggestions: Suggestion[] | null;
    detectorOk: boolean | null;
    suggestionError: string | null;
    deciding: boolean;
    locale: Locale;
    t: Translate;
    scanning: boolean;
    scanReport: ScanReport | null;
    scanError: string | null;
    onScan: (folder: string) => Promise<void>;
    onLocale: (locale: Locale) => void;
    onOpenFolder: (path: string) => void;
    onOpenDocument: (path: string) => void;
    onDecide: (id: number, accept: boolean) => Promise<void>;
    removal: FolderRemoval | null;
    onAskRemoval: (id: number) => void;
    onConfirmRemoval: () => void;
    onCancelRemoval: () => void;
    refusal: FolderRefusal | null;
    onCloseRefusal: () => void;
    inTauri: boolean;
  } = $props();

  const providerMessage = $derived(describeProviderRegistry(providers, t));
  const folderMessage = $derived(describeWatchedFolders(folders, t));
  const suggestionMessage = $derived(
    describeSuggestions(suggestions, t, detectorOk),
  );

  let folder = $state("");

  // R11: the primary route is the system's own folder browser, so a person does not have to know where
  // a folder lives in order to nominate it. It is a command of ours rather than the dialog plugin's own
  // JavaScript API, for this project's usual reason: the screens call our thin commands and the
  // decision stays on the Rust side, where it is testable without a display.
  async function pickFolder(): Promise<void> {
    if (!inTauri) {
      return;
    }
    try {
      const picked = await invoke<string | null>("pick_folder");
      // Cancelling is a normal answer rather than a mistake: it changes nothing and says nothing.
      if (picked !== null) {
        folder = picked;
      }
    } catch (error) {
      // A dialog that cannot open at all is a real failure, and there is no screen for it here.
      console.error("the folder chooser could not be opened", error);
    }
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const path = folder.trim();
    if (path === "") {
      return;
    }
    // R22 is decided where the watched folders are held, because the refusal is a fact about the store
    // rather than about this form: the screen hands the path over and the answer comes back as a prop.
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
            <!-- R24: the path itself is the control, so clicking a folder's own address opens it in the
                 file manager — which is what a person means when they click a folder. R21: it is set at a
                 secondary size, because it is the least important thing on this row and it used to read
                 as the most important. -->
            <button
              type="button"
              class="watched-path"
              title={t("settings.folders.open", { path: watched.path })}
              onclick={() => onOpenFolder(watched.path)}
            >{watched.path}</button>
            <span class="detail">
              {watched.recursive
                ? t("settings.folders.recursive")
                : t("settings.folders.notRecursive")},
              {watched.enabled
                ? t("settings.folders.enabled")
                : t("settings.folders.disabled")}
            </span>
            <!-- R27: this used to need no confirmation, because removing a watch cost the watching and
                 nothing else. It now takes the folder's documents out of the library with it, so it is a
                 destructive action and it asks first: the click opens the plan, and the modal states how
                 many documents leave before anything happens. -->
            <button
              type="button"
              class="remove"
              aria-label={t("settings.folders.removeFolder", { path: watched.path })}
              onclick={() => onAskRemoval(watched.id)}
            >{t("settings.folders.remove")}</button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <!--
    L56, restated in L57 after the user's verdict on the first attempt: a suggestion is shown as a CARD,
    the same shape as a library card — the cover's reserved space and the text block under it — because
    it belongs to the same family of things, and it read as a different kind of object before. The two
    decisions sit INSIDE the cover's space, which is the document's own body and the same place a card's
    edit and folder actions already live, and they are ICONS: an icon is not a name, so each button's
    accessible name comes from the catalogue through \`aria-label\`, exactly as the card's own icon
    actions do. The heading is the file's own name and never the path; the path is stated under it.
  -->
  <div class="panel">
    <h2>{t("suggestion.heading")}</h2>
    <p class="state">{t("suggestion.definition")}</p>
    {#if suggestionError !== null}
      <p class="state error">{t("suggestion.error", { message: suggestionError })}</p>
    {:else if suggestionMessage !== null}
      <p class="state">{suggestionMessage}</p>
    {:else}
      <div class="suggestions">
        {#each suggestions ?? [] as suggestion (suggestion.id)}
          <article>
            <SuggestionCover
              id={suggestion.id}
              path={suggestion.path}
              {t}
              {deciding}
              onDecide={onDecide}
            />
            <div class="body">
              <h3 class="suggestion-title" title={suggestion.path}>
                {suggestionTitle(suggestion.path)}
              </h3>
              <button
                type="button"
                class="suggestion-path"

                title={t("suggestion.openFolder", { path: suggestion.path })}
                aria-label={t("suggestion.openFolder", { path: suggestion.path })}
                onclick={() => onOpenDocument(suggestion.path)}
              >{suggestion.path}</button>
            </div>
          </article>
        {/each}
      </div>
    {/if}
  </div>

  <div class="panel">
    <h2>{t("scan.heading")}</h2>
    <form onsubmit={submit}>
      <!--
        R25: the section is composed rather than stacked. The folder being acted on is the subject, so it
        reads as a field-like readout and not as a sentence; the two things a person can DO with it sit
        together in one row; and the manual route is a quiet link, because it is another way to fill the
        same field rather than a box of its own. What was here before was six blocks of equal weight —
        two identical buttons, the path, its line, the browser's disclosure triangle and the alert —
        which is what made the section read as a list of unrelated things.
      -->
      <p class="field" aria-live="polite">
        {#if folder === ""}
          <span class="empty">{t("scan.noFolder")}</span>
        {:else}
          {folder}
        {/if}
      </p>

      <div class="actions">
        {#if inTauri}
          <button type="button" class="secondary" onclick={pickFolder}>{t("scan.choose")}</button>
        {/if}
        <!-- The scan is the action this panel exists for, so it is the prominent one — and it stays
             disabled until there is a path, which is the shape of every form a person already knows. -->
        <button type="submit" disabled={scanning || folder.trim() === ""}>{t("scan.action")}</button>
      </div>

      <details class="manual">
        <summary>{t("scan.manual")}</summary>
        <input
          bind:value={folder}
          placeholder={t("scan.placeholder")}
          aria-label={t("scan.folderLabel")}
        />
      </details>
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

<!--
  R26: what used to be a line under the form is a modal now, and the same modal serves both things that
  need one. Only one can be open at a time, and the removal wins the tie: it is the one that was asked
  for by a click and the one that is waiting on an answer.
-->
{#if removal !== null}
  <Modal title={t("settings.folders.removalTitle")} onClose={onCancelRemoval}>
    <!-- The modal names its own subject: with two folders watched, "this folder" without a path would
         leave the reader to remember which button they pressed. -->
    <p class="detail path">{removal.folder.path}</p>
    <p>{describeRemoval(removal, t)}</p>
    <!-- Only when the removal is partial: with nothing leaving, the sentence above has already said
         that the documents are covered by another watch, and following it with "documents another
         watched folder covers stay" would repeat it. -->
    {#if removal.kept.length > 0 && removal.documents.length > 0}
      <p class="detail">{t("settings.folders.removalKept")}</p>
    {/if}
    {#if removal.documents.length > 0}
      <p class="detail">{t("settings.folders.removalRecover")}</p>
    {/if}
    <div class="actions">
      <button type="button" class="secondary" onclick={onCancelRemoval}>
        {t("settings.folders.removalCancel")}
      </button>
      <button type="button" class="danger" onclick={onConfirmRemoval}>
        {t("settings.folders.removalConfirm")}
      </button>
    </div>
  </Modal>
{:else if refusal !== null}
  <Modal title={t("scan.refusedTitle")} onClose={onCloseRefusal}>
    <p>{describeCoveredFolder(refusal.covered, refusal.candidate, t)}</p>
    <div class="actions">
      <button type="button" onclick={onCloseRefusal}>{t("scan.refusedClose")}</button>
    </div>
  </Modal>
{/if}

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

  /* R21 and R24: the watched folder's address is a control (it opens the folder) and reads as the least
     important thing on the row rather than the loudest. */
  .watched-path {
    background: none;
    border: 0;
    padding: 0;
    color: #24506b;
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    text-decoration: underline;
    text-underline-offset: 0.15em;
    cursor: pointer;
    overflow-wrap: anywhere;
  }

  .watched-path:hover {
    color: #17364a;
  }

  /* R27: the way to stop watching a folder, which now takes the folder's documents with it. It is still
     a small secondary control because the confirmation, not the button, is where the consequence is
     stated — which is the point of asking. */
  .remove {
    background: none;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    padding: 0.15rem 0.45rem;
    color: #a1301f;
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
    align-self: flex-start;
  }

  .remove:hover {
    background: #fdf1ef;
  }

  /* R25: the folder the panel acts on, as a readout. It is styled like a field because that is what it
     is — the value the actions below apply to — and it wraps anywhere, because an absolute path is long
     and pushing the layout around is worse than breaking a line. */
  .field {
    margin: 0;
    padding: 0.45rem 0.6rem;
    border: 1px solid #e2e6eb;
    border-radius: 0.4rem;
    background: #f7f9fb;
    font-size: 0.82rem;
    color: #1c2024;
    overflow-wrap: anywhere;
    box-sizing: border-box;
    width: 100%;
  }

  .field .empty {
    color: #8b95a3;
  }

  .detail {
    font-size: 0.8rem;
    color: #5a6472;
  }

  /* L57: the suggestion cards, shaped like the library's. Two columns, because the window is 430 wide
     and a card needs the room for its heading; nothing here scrolls sideways. */
  .suggestions {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.7rem;
    margin-top: 0.7rem;
  }

  .suggestions article {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }


  /* Two lines at most, like a library card's title, so two suggestions never differ in height and the
     block under the cover keeps one shape down the row. The whole name is still there for a pointer
     through \`title=\` and for a screen reader. */
  /* L58: the path is a small control that opens the document's own folder, the way a library card's
     folder action does — a path a person cannot use is a dead line on the card. */
  .suggestion-path {
    background: none;
    border: 0;
    padding: 0;
    text-align: left;
    font-size: 0.7rem;
    color: #5a6472;
    cursor: pointer;
    overflow-wrap: anywhere;
  }

  .suggestion-path:hover {
    color: #17364a;
    text-decoration: underline;
  }

  .suggestion-title {
    margin: 0;
    font-size: 0.9rem;
    line-height: 1.2;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .path {
    overflow-wrap: anywhere;
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
    flex-direction: column;
    align-items: stretch;
    gap: 0.6rem;
  }

  /* R25: the two things a person can do here, side by side, so they read as one set of actions rather
     than as two unrelated rows of the same weight. */
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  /* The manual route: a quiet link, because it is another way to fill the field above and not a block of
     its own. The browser's own disclosure triangle is removed rather than shown — it was one of the
     things that made this section read as a pile. */
  .manual summary {
    list-style: none;
    cursor: pointer;
    color: #24506b;
    font-size: 0.8rem;
    text-decoration: underline;
    text-underline-offset: 0.15em;
    width: fit-content;
  }

  .manual summary::-webkit-details-marker {
    display: none;
  }

  .manual input {
    margin-top: 0.4rem;
    width: 100%;
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

  button.secondary {
    background: #ffffff;
    color: #24506b;
  }

  button.danger {
    background: #a1301f;
    border-color: #a1301f;
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
