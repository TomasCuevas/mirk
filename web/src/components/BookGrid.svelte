<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    announceLibraryChanged,
    displayTitle,
  titleEditor,
    type FolderActionStatus,
    type LibraryEntry,
  } from "../lib/library";
  import { invoke, runningInTauri } from "../lib/tauri";
  import BookCard from "./BookCard.svelte";
  import Modal from "./Modal.svelte";

  let {
    entries,
    t,
    folderStatus,
    onOpenFolder,
    layout = "grid",
  }: {
    entries: readonly LibraryEntry[];
    t: Translate;
    folderStatus: FolderActionStatus | null;
    onOpenFolder: (entry: LibraryEntry) => void;
    layout?: "grid" | "strip";
  } = $props();

  /**
   * How many tiles a carousel shows before it stops (R16).
   *
   * A strip is a preview, not a list: with a whole library inside it, it stops being a carousel — the
   * user's words were that too many tiles made one so long it was a nuisance to look at. Ten is the
   * number the library already paginates by, so "abrir la lista completa" is where the rest belongs
   * and the two limits teach the same expectation instead of two different ones.
   */
  const CAROUSEL_TILES = 10;

  // A strip shows the first tiles only. A grid shows everything it was given, because a full list is
  // the one screen that is supposed to be complete.
  const shown = $derived(layout === "strip" ? entries.slice(0, CAROUSEL_TILES) : entries);

  // R31. One editor for the whole grid rather than one per card, so a screen with many cards still
  // owns a single dialog. The card opens it through `onEditTitle`; the decision of what a title means
  // lives in the core, and this component only carries the reader's text there and asks for the
  // listing to be re-read afterwards. It lives here rather than in a screen because the category
  // screen renders this same grid and cannot be given a new screen-level callback.
  let editing = $state<LibraryEntry | null>(null);
  let draft = $state("");
  let saving = $state(false);
  let titleError = $state<string | null>(null);

  function openTitleEditor(entry: LibraryEntry): void {
    editing = entry;
    draft = titleEditor(entry).value;
    titleError = null;
  }

  function closeTitleEditor(): void {
    if (saving) {
      return;
    }
    editing = null;
  }

  async function saveTitle(): Promise<void> {
    const value = draft.trim();
    if (value === "") {
      // The core refuses a blank title too; this is the sentence the reader sees, in their language.
      titleError = t("card.titleBlank");
      return;
    }
    await sendTitle(value);
  }

  async function restoreTitle(): Promise<void> {
    // `null` is "restore the document's own title": a legitimate action, not a blank one.
    await sendTitle(null);
  }

  async function sendTitle(title: string | null): Promise<void> {
    if (editing === null) {
      return;
    }
    if (!runningInTauri()) {
      // Outside the desktop shell there is no store to write to. Saying so is better than closing the
      // dialog as if the title had been saved.
      titleError = t("app.outsideTauri");
      return;
    }
    saving = true;
    titleError = null;
    try {
      await invoke("set_document_title", { id: editing.id, title });
      editing = null;
      announceLibraryChanged();
    } catch (error) {
      titleError = String(error);
    } finally {
      saving = false;
    }
  }
</script>

<!--
  The one place cards are wired up (B2). The library, a category's full list and Inicio's carousels
  all render this, so "the same cards as the library" is a fact of the code rather than three copies
  of the same markup that can drift. `layout` only changes how the list is laid out: a wrapping grid
  for a full list, a horizontal scrolling strip for a carousel (B1).
-->
<ul class="grid" class:strip={layout === "strip"}>
  {#each shown as entry (entry.id)}
    <li>
      <BookCard
        {entry}
        {t}
        {onOpenFolder}
        onEditTitle={openTitleEditor}
        status={folderStatus?.id === entry.id ? folderStatus : null}
      />
    </li>
  {/each}
</ul>

<!-- R31: the title editor, one per grid. Its field is prefilled with the DISPLAYED title — the
     reader's when they set one, the document's own otherwise — and the restore control appears only
     when the reader's title is set, because restoring something that was never set is an action
     without a meaning. -->
{#if editing !== null}
  {@const editor = titleEditor(editing)}
  <Modal title={t("card.titleEditor")} onClose={closeTitleEditor}>
    <label class="title-field">
      <span>{t(editor.yours ? "card.titleLabelYours" : "card.titleLabel")}</span>
      <input
        type="text"
        bind:value={draft}
        aria-label={t(editor.yours ? "card.titleLabelYours" : "card.titleLabel")}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            void saveTitle();
          }
        }}
      />
    </label>

    {#if titleError !== null}
      <p class="title-error" role="alert">{titleError}</p>
    {/if}

    <!-- Two ROWS, and that is a decision rather than a wrapped line. Restore is a different kind of
         action from the pair below it: Cancel and Save apply or discard the DRAFT, while Restore throws
         the reader's title away and writes immediately. Sharing one row also broke as soon as the label
         was long enough — the user's own screenshots show "Guardar" falling onto a second line with the
         pair left unpaired. Two rows say what is true, and no width can re-break it. -->
    <div class="title-actions">
      {#if editor.yours}
        <div class="restore-row">
          <button type="button" class="restore" onclick={() => void restoreTitle()} disabled={saving}>
            {t("card.titleRestore")}
          </button>
        </div>
      {/if}
      <div class="confirm-row">
        <button type="button" onclick={closeTitleEditor} disabled={saving}>
          {t("card.titleCancel")}
        </button>
        <button type="button" onclick={() => void saveTitle()} disabled={saving}>
          {t("card.titleSave")}
        </button>
      </div>
    </div>
  </Modal>
{/if}

<style>
  /* One gap for both axes, which is what the design asked for from the start: the rows looked
     unseparated because the card overflowed its own grid row (see BookCard, where
     `box-sizing: border-box` is the line that fixes it), not because the gap was missing. */
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(8.5rem, 1fr));
    column-gap: 0.75rem;
    row-gap: 0.75rem;
  }

  .grid li {
    display: flex;
  }

  /* B1: a carousel scrolls sideways rather than wrapping. The tiles keep a fixed width so the strip
     has a definite end to scroll to, and `overflow-x: auto` is the scroll itself. */
  .grid.strip {
    display: flex;
    flex-wrap: nowrap;
    overflow-x: auto;
    /* R17: as soon as one axis is not `visible`, the other computes to `auto` too — so `overflow-x`
       alone silently enabled a vertical scrollbar, which showed for the overflow described in
       BookCard's fixed-height body. The strip scrolls sideways and only sideways, and this says so
       rather than leaving it to a CSS default; the overflow itself was fixed where it happened. */
    overflow-y: hidden;
    gap: 0.75rem;
    padding-bottom: 0.35rem;
  }

  .grid.strip li {
    /* The tile width is the carousel's own, inherited as `--tile`, so a card and an empty slot are
       the same size by construction. The fallback covers a strip used without that variable. */
    flex: 0 0 var(--tile, 8.5rem);
  }

  /* R31: the title editor's own chrome, on top of the shared modal. */
  .title-field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.8rem;
    color: #5a6472;
  }

  .title-field input {
    padding: 0.5rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    font: inherit;
    font-size: 0.95rem;
    color: #1c2024;
    box-sizing: border-box;
  }

  .title-error {
    margin: 0;
    font-size: 0.8rem;
    color: #a1301f;
  }

  .title-actions {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .restore-row,
  .confirm-row {
    display: flex;
    gap: 0.5rem;
  }

  .confirm-row {
    justify-content: flex-end;
  }

  .title-actions button {
    padding: 0.4rem 0.7rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .title-actions button:hover:not(:disabled) {
    background: #f0f2f5;
  }

  .title-actions button:disabled {
    color: #9aa3af;
    cursor: default;
  }

  /* The restore action restores rather than destroys, so it reads as a secondary choice: it is
     pushed to the left, away from the cancel/save pair. */
  .title-actions .restore {
    margin-right: auto;
  }
</style>
