<script lang="ts">
  import { untrack } from "svelte";
  import type { Translate } from "../lib/i18n";
  import {
    announceLibraryChanged,
    authorEditor,
    coverEditor,
    coverPageFromInput,
    editorClosesAfterCommit,
    editorEntry,
    editorRefreshesLibrary,
    fetchCover,
    libraryAuthors,
    titleEditor,
    type EditorCommit,
    type LibraryEntry,
  } from "../lib/library";
  import { invoke, runningInTauri } from "../lib/tauri";
  import BookCard from "./BookCard.svelte";
  import Modal from "./Modal.svelte";

  let {
    entries,
    authors = null,
    t,
    onOpenFolder,
    onOpenDocument,
    layout = "grid",
    initialEditing = null,
  }: {
    entries: readonly LibraryEntry[];
    /**
     * The authors the editor offers (R40), computed by the screen that holds the WHOLE listing.
     *
     * The grid cannot derive this itself: a screen hands it one page (ten documents) or one carousel
     * slice, so an author whose only document sat elsewhere did not exist — the user's own report,
     * "solo aparecen los autores que esten en los libros de la pagina actual". `null` means "derive it
     * from the entries this grid holds", which is the sensible answer for a grid rendered on its own
     * (a test), and never the right answer for a paged screen.
     */
    authors?: readonly string[] | null;
    t: Translate;
    onOpenFolder: (entry: LibraryEntry) => void;
    /**
     * Opens a document in the system's own application for it (R37).
     *
     * The grid carries it the same way it carries the folder action, so a card in Inicio's carousel
     * and the same card in the library open the document the same way.
     */
    onOpenDocument: (entry: LibraryEntry) => void;
    layout?: "grid" | "strip";
    /**
     * The document whose editor is already open when this grid is rendered, or `null`. The grid opens
     * the editor from a card's pencil in the running application; this prop is the render seam a test
     * needs to put the modal on screen without a DOM, the same kind of seam as BookCard's `cover`.
     */
    initialEditing?: LibraryEntry | null;
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

  // R31/R32. One editor for the whole grid rather than one per card, so a screen with many cards
  // still owns a single dialog. The card opens it through `onEditDocument`, and it is the ONE editing
  // surface for a document: its title (R31) and its cover (R32). The decision of what a title or a
  // cover means lives in the core and in `library.ts`; this component only carries the reader's answer
  // there and asks for the listing to be re-read afterwards. It lives here rather than in a screen
  // because the category screen renders this same grid and cannot be given a new screen-level
  // callback.
  // The editor's initial state comes from `initialEditing` ONCE (see the prop's note): the editor does
  // not follow the prop afterwards, it is opened and closed by the cards. `untrack` says that on
  // purpose, so the compiler does not read the one-time capture as a mistake. It is reconciled with
  // the current listing by id first, so a snapshot from before a cover change cannot seed an empty
  // page field or the document's own title over the reader's.
  const seed = untrack(() => {
    const opened =
      initialEditing === null ? null : editorEntry(entries, initialEditing);
    return {
      entry: opened,
      draft: opened === null ? "" : titleEditor(opened).value,
      author: opened === null ? "" : authorEditor(opened).value,
      page: opened === null ? "" : coverEditor(opened).page,
      hasImage: opened === null ? false : coverEditor(opened).hasImage,
    };
  });
  let editing = $state<LibraryEntry | null>(seed.entry);
  let draft = $state(seed.draft);
  // The reader's author draft, decided by the same Cancel/Save pair as the title. An EMPTY value is
  // legitimate here ("no author of my own") and is sent as no value, unlike the title — the two fields
  // share the pair of controls but not the blank rule.
  let authorDraft = $state(seed.author);
  let saving = $state(false);
  let titleError = $state<string | null>(null);
  // R32: the cover's own draft and error, in the same editor as the title.
  let coverPageDraft = $state(seed.page);
  // R32, corrected: whether there is an image to remove. The editor now STAYS OPEN when a cover
  // action succeeds, so it cannot keep reading the `editing` entry for this: the entry is the one
  // that was open, and the listing refresh below is asynchronous. The actions keep this answer current
  // themselves, so the remove control appears and disappears with the image it acts on rather than
  // lying while the editor is up.
  let coverHasImage = $state(seed.hasImage);
  let coverError = $state<string | null>(null);
  let coverSaving = $state(false);

  // The facts the editor STATES — whose title this is, and the document the field belongs to — are read
  // from the current listing by id, so the refresh a completed cover change now triggers (exactly as a
  // title change does) keeps them current. The drafts the reader is typing stay in their own state and
  // are never rewritten by a refresh, which is what keeps an undecided title from being discarded.
  const shownEditing = $derived(editing === null ? null : editorEntry(entries, editing));

  // The authors the editor's list offers. Computed by the screen where the whole listing lives and
  // handed down (R40/S1); the fallback keeps a grid rendered on its own usable, and it is the WHY of
  // this prop: a page of ten cannot know the library's authors.
  const authorSuggestions = $derived(authors ?? libraryAuthors(entries));

  // The list is a PICKER, not a second copy of the value: choosing a name fills the draft and the list
  // goes back to its "choose an author" entry, so the reader can clear the field and pick the same name
  // again — a select that still showed it would send no `change` on the second pick. The value stays
  // where the reader can edit it, in the text field (S2: typing a brand-new author keeps working).
  function chooseAuthor(list: HTMLSelectElement): void {
    const chosen = list.value;
    list.value = "";
    if (chosen !== "") {
      authorDraft = chosen;
    }
  }

  function openDocumentEditor(entry: LibraryEntry): void {
    // The document may have changed on another screen since the card was drawn, so the editor seeds
    // from the current listing entry rather than the card's snapshot.
    const current = editorEntry(entries, entry);
    editing = current;
    draft = titleEditor(current).value;
    authorDraft = authorEditor(current).value;
    coverPageDraft = coverEditor(current).page;
    coverHasImage = coverEditor(current).hasImage;
    titleError = null;
    coverError = null;
  }

  function closeDocumentEditor(): void {
    if (saving || coverSaving) {
      return;
    }
    editing = null;
  }

  // The ONE place a successful commit's effect on the editor is applied. Whether a commit closes it
  // and whether it refreshes the listing are rules that live in `library.ts` (R32, corrected) so they
  // are testable; this only obeys the answers, and both halves of the editor read the same answers
  // rather than each deciding for itself.
  function settleEditorAfter(commit: EditorCommit): void {
    if (editorClosesAfterCommit(commit)) {
      editing = null;
    }
    if (editorRefreshesLibrary(commit)) {
      announceLibraryChanged();
    }
  }

  async function saveTitle(): Promise<void> {
    const value = draft.trim();
    if (value === "") {
      // The core refuses a blank title too; this is the sentence the reader sees, in their language.
      titleError = t("card.titleBlank");
      return;
    }
    // The author's empty value is legitimate — no author of my own — so it is sent as an EMPTY value
    // (`null`, which the command reads as "clear the reader's author") rather than refused, and it falls
    // back to the document's own author. Absent is a different thing and this control never sends it: it
    // means "say nothing about the author", which is Restore's decision alone. This is the deliberate
    // difference from the title above, which refuses a blank because a document must have a title.
    const author = authorDraft.trim();
    await sendTitle(value, author === "" ? null : author);
  }

  async function restoreTitle(): Promise<void> {
    // `null` is "restore the document's own title": a legitimate action, not a blank one. It does not
    // touch the author draft, which belongs to the same Save/Cancel pair.
    await sendTitle(null);
  }

  // ONE command and ONE store write, because Save decides two facts in one action: the title and the
  // author. Written as two commands it was two writes, and a failure between them left the title saved
  // while the screen reported an error implying nothing had been — a half-applied save.
  //
  // `author` absent (`undefined`, which is what `JSON.stringify` drops from the arguments) means this
  // action said NOTHING about the author, and the reader's author is left exactly as it was. That is
  // Restore's decision, which restores the title and nothing else. `null` is different on purpose: it is
  // Save saying "no author of my own", which clears the reader's author and falls back to the
  // document's own. The `?` and the `| null` are that distinction, and the Rust command reads it the
  // same way.
  async function sendTitle(title: string | null, author?: string | null): Promise<void> {
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
      await invoke("set_document_naming", { id: editing.id, title, author });
      settleEditorAfter("title");
    } catch (error) {
      // The message can be taken at its word: the write is one operation, so a failure means neither
      // fact was applied and there is nothing half-saved for this sentence to lie about.
      titleError = String(error);
    } finally {
      saving = false;
    }
  }

  // --- R32: the cover, inside the same editor -------------------------------------------------
  //
  // The decision of what a cover means (which page, which image) lives in the core and in
  // `library.ts`; this component only carries the reader's answer there.

  // The cover a card should show after the reader changed it (R32). A card asks for its own cover on
  // mount; when the choice changes, the grid hands the new answer down so the card does not have to
  // know a change happened. An entry that is absent here has not changed and still asks for itself.
  let coverOverrides = $state<Record<number, string | null>>({});

  // Every cover action has the same tail: re-read the cover, hand it to the card, keep the editor's
  // own answer for the remove control, and let `settleEditorAfter` apply the rules — including the
  // listing refresh a completed title change already performed. The image path and the page path MUST
  // not differ here: that difference is what left the reader's title stale after an image and the
  // chosen page empty after a page. Keeping ONE tail is what stops them from disagreeing again.
  async function finishCoverChange(entry: LibraryEntry, imageState?: boolean): Promise<void> {
    const source = await fetchCover(entry, invoke);
    coverOverrides = { ...coverOverrides, [entry.id]: source };
    if (imageState !== undefined) {
      coverHasImage = imageState;
    }
    settleEditorAfter("cover");
  }

  async function sendCoverChange(
    operation: () => Promise<unknown>,
    imageState?: boolean,
  ): Promise<void> {
    const entry = editing;
    if (entry === null) {
      return;
    }
    if (!runningInTauri()) {
      coverError = t("app.outsideTauri");
      return;
    }
    coverSaving = true;
    coverError = null;
    try {
      await operation();
      await finishCoverChange(entry, imageState);
    } catch (error) {
      coverError = String(error);
    } finally {
      coverSaving = false;
    }
  }

  async function saveCoverPage(): Promise<void> {
    const choice = coverPageFromInput(coverPageDraft);
    if (!choice.ok) {
      // The store accepts any page; this is the sentence the reader sees, in their language.
      coverError = t("card.coverPageInvalid");
      return;
    }
    await sendCoverChange(() =>
      invoke("set_cover_page", { id: editing?.id, page: choice.page }),
    );
  }

  async function chooseCoverImage(): Promise<void> {
    // The command opens the native dialog. `false` is a cancel — a normal answer — so the modal stays
    // open and nothing is refreshed; only a real failure becomes an error sentence. A real choice runs
    // the SAME tail as a page change, so it cannot refresh the interface differently from it.
    const entry = editing;
    if (entry === null) {
      return;
    }
    if (!runningInTauri()) {
      coverError = t("app.outsideTauri");
      return;
    }
    coverSaving = true;
    coverError = null;
    try {
      const installed = await invoke<boolean>("choose_cover_image", { id: entry.id });
      if (installed) {
        await finishCoverChange(entry, true);
      }
    } catch (error) {
      coverError = String(error);
    } finally {
      coverSaving = false;
    }
  }

  async function removeCoverImage(): Promise<void> {
    await sendCoverChange(() => invoke("remove_cover_image", { id: editing?.id }), false);
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
        {onOpenDocument}
        onEditDocument={openDocumentEditor}
        cover={coverOverrides[entry.id]}
      />
    </li>
  {/each}
</ul>

<!-- R31/R32: the document editor, one per grid, and the ONE surface where a document is edited. Its
     title field is prefilled with the DISPLAYED title — the reader's when they set one, the document's
     own otherwise — and the restore control appears only when the reader's title is set, because
     restoring something that was never set is an action without a meaning. The author field is its
     twin: prefilled with the author on screen, with the library's own authors as native suggestions,
     and an empty value meaning "no author of my own".

     The cover lives here too: a page of the reader's own (the first page is the plain default, and an
     empty field means it) and an image of their own, with the remove control appearing only once an
     image is set — removing something that was never set is an action without a meaning. The image is
     chosen through the native dialog from the Rust side; this editor only asks. It sits at the foot of
     the one modal rather than in a modal of its own, because the user rejected a second control over a
     small cover and the pencil already means "edit this document". -->
{#if shownEditing !== null}
  {@const editor = titleEditor(shownEditing)}
  {@const author = authorEditor(shownEditing)}
  <Modal title={t("card.documentEditor")} onClose={closeDocumentEditor}>
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

    <!-- The author, in the SAME editor and decided by the same Cancel/Save pair as the title: the user
         asked for it behind the pencil, never as another control on the card. Its empty value is
         legitimate — leave it empty to fall back to the document's own author — which is the deliberate
         difference from the title's blank refusal.

         R40/S2: the list of names is a real `<select>` beside the field, not a `<datalist>`. WebKitGTK —
         the engine this window actually renders in — never implemented the `datalist` popup, so even a
         complete list opened nothing. The field stays editable, because the reader must still be able to
         type an author nobody in the library has yet. -->
    <label class="title-field">
      <span>{t(author.yours ? "card.authorLabelYours" : "card.authorLabel")}</span>
      <div class="author-row">
        <input
          type="text"
          bind:value={authorDraft}
          aria-label={t(author.yours ? "card.authorLabelYours" : "card.authorLabel")}
          onkeydown={(event) => {
            if (event.key === "Enter") {
              void saveTitle();
            }
          }}
        />
        <select
          aria-label={t("card.authorChoose")}
          onchange={(event) => chooseAuthor(event.currentTarget)}
        >
          <option value="">{t("card.authorChoose")}</option>
          {#each authorSuggestions as suggestion (suggestion)}
            <option value={suggestion}>{suggestion}</option>
          {/each}
        </select>
      </div>
    </label>
    <p class="cover-hint">{t("card.authorHint")}</p>

    <!-- Two ROWS, and that is a decision rather than a wrapped line. Restore is a different kind of
         action from the pair below it: Cancel and Save apply or discard the DRAFT, while Restore throws
         the reader's title away and writes immediately. Sharing one row also broke as soon as the label
         was long enough — the user's own screenshots show "Guardar" falling onto a second line with the
         pair left unpaired. Two rows say what is true, and no width can re-break it. -->
    <div class="title-actions">
      {#if editor.yours}
        <div class="restore-row">
          <button
            type="button"
            class="restore"
            onclick={() => void restoreTitle()}
            disabled={saving || coverSaving}
          >
            {t("card.titleRestore")}
          </button>
        </div>
      {/if}
      <div class="confirm-row">
        <button type="button" onclick={closeDocumentEditor} disabled={saving || coverSaving}>
          {t("card.titleCancel")}
        </button>
        <button type="button" onclick={() => void saveTitle()} disabled={saving || coverSaving}>
          {t("card.titleSave")}
        </button>
      </div>
    </div>

    <!-- R32: the cover, the second half of the one editor. The heading states which part of the
         document the controls below act on. -->
    <h4 class="cover-heading">{t("card.coverEditor")}</h4>
    <label class="cover-field">
      <span>{t("card.coverPageLabel")}</span>
      <input
        type="number"
        min="1"
        bind:value={coverPageDraft}
        aria-label={t("card.coverPageLabel")}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            void saveCoverPage();
          }
        }}
      />
    </label>
    <p class="cover-hint">{t("card.coverPageHint")}</p>

    {#if coverError !== null}
      <p class="title-error" role="alert">{coverError}</p>
    {/if}

    <div class="title-actions">
      <div class="confirm-row">
        <button type="button" onclick={() => void saveCoverPage()} disabled={saving || coverSaving}>
          {t("card.coverPageSave")}
        </button>
      </div>
      <div class="confirm-row">
        <button
          type="button"
          onclick={() => void chooseCoverImage()}
          disabled={saving || coverSaving}
        >
          {t("card.coverChooseImage")}
        </button>
        {#if coverHasImage}
          <button
            type="button"
            onclick={() => void removeCoverImage()}
            disabled={saving || coverSaving}
          >
            {t("card.coverRemoveImage")}
          </button>
        {/if}
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

  /* R40: the field and the list beside it. The field takes the room, because typing stays the primary
     gesture and the list is the shortcut for a name the library already knows. */
  .author-row {
    display: flex;
    gap: 0.4rem;
  }

  .author-row input {
    flex: 1 1 auto;
    min-width: 0;
  }

  .author-row select {
    flex: 0 1 auto;
    max-width: 45%;
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

  /* R32: the cover half of the one editor. The heading separates it from the title above without a
     second modal, and it names what the controls below act on. */
  .cover-heading {
    margin: 0.2rem 0 0;
    font-size: 0.85rem;
    color: #1c2024;
  }

  /* R32: the cover editor's page field and its hint, sharing the title field's chrome. */
  .cover-field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.8rem;
    color: #5a6472;
  }

  .cover-field input {
    padding: 0.5rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    font: inherit;
    font-size: 0.95rem;
    color: #1c2024;
    box-sizing: border-box;
  }

  .cover-hint {
    margin: 0;
    font-size: 0.75rem;
    color: #5a6472;
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
