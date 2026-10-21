<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { Translate } from "../lib/i18n";
  import {
    addAuthorChip,
    announceLibraryChanged,
    authorChoices,
    authorEditor,
    coverEditor,
    coverPageFromInput,
    coverPageText,
    coverSource,
    editorEntry,
    fetchCover,
    libraryAuthors,
    removeAuthorChip,
    sameCoverPage,
    titleEditor,
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

  // The carousel's own controls (R15). A strip scrolls sideways, and a sideways scroll that is only
  // discoverable by dragging or by a scrollbar is a strip the reader will not scroll — the user asked
  // for arrows and for the scrollbar to go. The arrows are the controls; the scrollbar is hidden and
  // the strip keeps its native scrolling for the pointer and the trackpad.
  let strip = $state<HTMLElement | null>(null);
  let atStart = $state(true);
  let atEnd = $state(false);

  /** What one step of an arrow covers: a tile plus the strip's own gap, so cards do not get cut. */
  function step(): number {
    if (strip === null) {
      return 0;
    }
    const tile = strip.firstElementChild;
    const width = tile instanceof HTMLElement ? tile.getBoundingClientRect().width : 0;
    return width > 0 ? width + 12 : strip.clientWidth * 0.8;
  }

  /** Whether either end is reached, so an arrow that would do nothing is not offered as if it would. */
  function measure(): void {
    if (strip === null) {
      return;
    }
    atStart = strip.scrollLeft <= 1;
    atEnd = strip.scrollLeft + strip.clientWidth >= strip.scrollWidth - 1;
  }

  function scrollTiles(direction: 1 | -1): void {
    if (strip === null) {
      return;
    }
    const reduced =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    strip.scrollBy({ left: direction * step(), behavior: reduced ? "auto" : "smooth" });
  }

  // A strip whose content changes (a carousel can gain a document while the screen is open) has new
  // ends, so the arrows are measured again after every render that changes the slice.
  $effect(() => {
    void shown.length;
    measure();
  });

  onMount(() => {
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  });

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
      authors: opened === null ? [] : authorEditor(opened).chips,
      page: opened === null ? "" : coverEditor(opened).page,
      hasImage: opened === null ? false : coverEditor(opened).hasImage,
      isImage: opened === null ? false : coverEditor(opened).source === "image",
      // R18: the baselines start where the editor starts, whether it was opened by a click or seeded
      // by a test through `initialEditing`. A baseline that only a click populated left the render
      // seam with an empty "saved" title, so an untouched editor looked dirty.
      savedTitle: opened === null ? "" : titleEditor(opened).value,
      savedChips: opened === null ? [] : authorEditor(opened).chips,
      savedPage: opened === null ? "" : coverEditor(opened).page,
    };
  });
  let editing = $state<LibraryEntry | null>(seed.entry);
  let draft = $state(seed.draft);
  // R42/U30: the author draft is the LIST the reader is building, one chip per author, in their order
  // (S3). The chips ARE the value — there is no single name field any more — and an EMPTY list is
  // legitimate here ("no author of my own"), unlike the title, which refuses a blank. The rules that
  // decide what a chip is (trim, blank, duplicate, append, remove) live in `library.ts`, because a
  // screen must not decide them.
  let authorChips = $state<string[]>(seed.authors);
  // The field where a NEW name arrives. It is not the value: it is empty between names, so the chip
  // list is the only place an author can be, and Enter in it adds a chip rather than saving.
  let authorInput = $state("");
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
  // R19: having an image and showing it are two different facts. The page field stays filled while
  // the image is the cover, and this is what says which one the card is drawing.
  let coverIsImage = $state(seed.isImage);
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
  // Renamed by R16: this is the list the field offers names FROM, not the suggestions themselves
  // (those are `choices`, narrowed by what the reader has typed).
  const knownAuthors = $derived(authors ?? libraryAuthors(entries));

  // The list is a PICKER, not a second copy of the value: choosing a name ADDS a chip and the list goes
  // back to its "choose an author" entry, so a name that was added and then removed can be added again
  // — a select that still showed it would send no `change` on the second pick (L70). Whether the name
  // is already on the list — or blank — is decided by `addAuthorChip`, never here.
  // R16: one field that both accepts a new name and offers the library's own. The user's correction:
  // *"el mismo input de texto debe ser un selector donde se pueda escribir"* — an input beside a
  // `<select>` made two gestures out of one idea, and the select was the ugliest thing in the editor.
  // The names offered are decided by `authorChoices`, so the field can only draw them.
    const choices = $derived(authorChoices(knownAuthors, authorInput, authorChips));
  let suggestionsOpen = $state(false);
  let highlighted = $state(-1);
  // R16: the editor shows the cover the card is actually showing, not a promise that one exists.
  let editorCover = $state<string | null>(null);

  // R18. What a save is compared against. The draft is the reader's; these are the values the store
  // already holds, and they are what makes "hay algo para guardar" a question with an answer. They are
  // updated by a successful commit, not by re-reading the listing, so the button disappears the moment
  // the store accepted the change rather than one refresh later.
  let savedTitle = $state(seed.savedTitle);
  let savedChips = $state<string[]>(seed.savedChips);
  let savedPage = $state(seed.savedPage);

  // R18: restoring is destructive, so it asks first.
  let restoreOpen = $state(false);

  const textDirty = $derived(
    draft !== savedTitle ||
      authorChips.length !== savedChips.length ||
      authorChips.some((name, index) => name !== savedChips[index]),
  );
  // R19: while the image is the cover, the page button is always worth offering — it is how the
  // reader takes the cover back to a page, number unchanged.
  // R20: compared as COVERS, not as text. One-based field, zero-based store, and a number input that
  // can hand back a number were three ways for "the same page" to look different.
  const coverDirty = $derived(!sameCoverPage(coverPageDraft, savedPage) || coverIsImage);

  function pickAuthor(name: string): void {
    addAuthor(name);
    highlighted = -1;
  }

  /** Arrow keys move inside the list; Enter takes the highlighted name or the typed one. */
  function authorKeydown(event: KeyboardEvent): void {
    const open = suggestionsOpen && choices.length > 0;

    if (event.key === "ArrowDown") {
      event.preventDefault();
      suggestionsOpen = true;
      highlighted = open ? Math.min(highlighted + 1, choices.length - 1) : 0;
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      highlighted = open ? Math.max(highlighted - 1, -1) : -1;
      return;
    }
    if (event.key === "Escape") {
      suggestionsOpen = false;
      highlighted = -1;
      return;
    }
    if (event.key === "Enter") {
      // Enter ADDS a name; it never saves the form. The highlighted suggestion wins when there is one,
      // which is what makes the keyboard path and the mouse path land on the same value.
      event.preventDefault();
      const choice = open && highlighted >= 0 ? choices[highlighted] : undefined;
      if (choice !== undefined) {
        pickAuthor(choice);
      } else {
        addAuthor(authorInput);
      }
      suggestionsOpen = true;
    }
  }

  // Adding appends at the end and refuses a blank or a name already on the list. Only an ACCEPTED name
  // clears the field: a refused one would otherwise look as if it had been taken, which is exactly what
  // the reader has to be able to see (`card.authorHint` says how to get back to the document's own).
  function addAuthor(name: string): void {
    const result = addAuthorChip(authorChips, name);
    if (result.added) {
      authorChips = result.chips;
      authorInput = "";
    }
  }

  function removeAuthor(name: string): void {
    authorChips = removeAuthorChip(authorChips, name);
  }

  function openDocumentEditor(entry: LibraryEntry): void {
    // The document may have changed on another screen since the card was drawn, so the editor seeds
    // from the current listing entry rather than the card's snapshot.
    const current = editorEntry(entries, entry);
    editing = current;
    draft = titleEditor(current).value;
    authorChips = authorEditor(current).chips;
    authorInput = "";
    coverPageDraft = coverEditor(current).page;
    coverHasImage = coverEditor(current).hasImage;
    coverIsImage = coverEditor(current).source === "image";
    savedTitle = titleEditor(current).value;
    savedChips = [...authorEditor(current).chips];
    savedPage = coverEditor(current).page;
    titleError = null;
    coverError = null;
    suggestionsOpen = false;
    highlighted = -1;
    restoreOpen = false;
    editorCover = null;
    // The cover is asked for on open rather than kept in the grid: a card may never have asked for
    // its own (a strip only draws ten of them), and a preview that says "no cover" about a document
    // that has one would be a lie in the one screen that exists to fix it.
    void fetchCover(current, invoke).then((source) => {
      if (editing?.id === current.id) {
        editorCover = coverSource(source);
      }
    });
  }

  function closeDocumentEditor(): void {
    if (saving || coverSaving) {
      return;
    }
    editing = null;
  }

  /** The confirmation answered: close it first, so the editor's own state is the only one on screen. */
  function confirmRestore(): void {
    restoreOpen = false;
    void restoreTitle();
  }

  /** Cancelar puts the draft back to what the store holds, and keeps the editor open. */
  function discardTextDraft(): void {
    draft = savedTitle;
    authorChips = [...savedChips];
    authorInput = "";
    titleError = null;
    suggestionsOpen = false;
  }

  // The ONE place a successful commit's effect on the editor is applied. Whether a commit closes it
  // and whether it refreshes the listing are rules that live in `library.ts` (R32, corrected) so they
  // are testable; this only obeys the answers, and both halves of the editor read the same answers
  // rather than each deciding for itself.
  /**
   * What a successful commit does now: it tells the owner of the listing to re-read it, and nothing
   * else. It used to close the editor after a title save, and the user asked for that to stop —
   * *"cuando se haga clic en Guardar no tendría que sacar automáticamente el formulario"*. The form
   * stays, the baseline moves, and the buttons disappear because there is nothing left to save.
   */
  function settleEditorAfter(): void {
    announceLibraryChanged();
  }

  async function saveTitle(): Promise<void> {
    const value = draft.trim();
    if (value === "") {
      // The core refuses a blank title too; this is the sentence the reader sees, in their language.
      titleError = t("card.titleBlank");
      return;
    }
    // The CHIPS are the value now (R42/U30), so a save sends the reader's ordered list itself: trimmed
    // and duplicate-free by construction, in the order the chips are drawn. An EMPTY list is still
    // legitimate — "no author of my own" — and is sent as `[]` rather than refused. An ABSENT argument
    // is a different thing and this control never sends it: it means "say nothing about the authors",
    // which is Restore's decision alone.
    await sendTitle(value, [...authorChips]);
  }

  async function restoreTitle(): Promise<void> {
    // `null` is "restore the document's own title": a legitimate action, not a blank one. It does not
    // touch the author draft, which belongs to the same Save/Cancel pair.
    await sendTitle(null);
  }

  // ONE command and ONE store write, because Save decides two facts in one action: the title and the
  // authors. Written as two commands it was two writes, and a failure between them left the title saved
  // while the screen reported an error implying nothing had been — a half-applied save.
  //
  // `authors` absent (`undefined`, which is what `JSON.stringify` drops from the arguments) means this
  // action said NOTHING about the authors, and the reader's list is left exactly as it was. That is
  // Restore's decision, which restores the title and nothing else. An EMPTY list is different on
  // purpose: it is Save saying "no author of my own", which clears the reader's list and lets the
  // document's own author show again. The `?` is that distinction, and the Rust command reads it the
  // same way.
  async function sendTitle(
    title: string | null,
    authors?: readonly string[],
  ): Promise<void> {
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
      await invoke("set_document_naming", { id: editing.id, title, authors });
      if (title === null) {
        // Restore is the one commit that leaves nothing to keep editing: the field would have to show
        // the document's own title, which this snapshot does not carry, so the editor closes and the
        // card shows the restored one. The confirmation says so before it happens.
        editing = null;
      } else {
        savedTitle = title;
        savedChips = [...authorChips];
      }
      settleEditorAfter();
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
  async function finishCoverChange(
    entry: LibraryEntry,
    imageState?: { has: boolean; active: boolean },
  ): Promise<void> {
    const source = await fetchCover(entry, invoke);
    coverOverrides = { ...coverOverrides, [entry.id]: source };
    // R17: the preview in the editor is the cover the document has NOW. It used to be read once, when
    // the editor opened, so a page change or a chosen image left the old picture on screen — the
    // user's report. The card and the preview are refreshed by the same answer, so they cannot
    // disagree about what the cover is.
    editorCover = source;
    if (imageState !== undefined) {
      coverHasImage = imageState.has;
      coverIsImage = imageState.active;
    }
    settleEditorAfter();
  }

  async function sendCoverChange(
    operation: () => Promise<unknown>,
    imageState?: { has: boolean; active: boolean },
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
    if (coverError === null) {
      // R20: the field and the baseline are both rewritten to the accepted page's canonical spelling.
      // Saving "03" leaves "3" on screen, and the button disappears because there is nothing left to
      // save — which is what the user asked for.
      const accepted = coverPageText(choice.page);
      savedPage = accepted;
      coverPageDraft = accepted;
      // R19: the accepted page is the cover now. The image, if there was one, is still stored.
      coverIsImage = false;
    }
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
        await finishCoverChange(entry, { has: true, active: true });
      }
    } catch (error) {
      coverError = String(error);
    } finally {
      coverSaving = false;
    }
  }

  /**
   * Brings the stored image back as the cover without choosing the file again (R19).
   *
   * It refuses only when there is no image to return to; the screen does not offer the control then,
   * so a refusal here means the store and the screen disagreed — which is the store's answer to give.
   */
  async function useStoredImage(): Promise<void> {
    await sendCoverChange(() => invoke("use_cover_image", { id: editing?.id }), {
      has: true,
      active: true,
    });
  }

  async function removeCoverImage(): Promise<void> {
    await sendCoverChange(() => invoke("remove_cover_image", { id: editing?.id }), {
      has: false,
      active: false,
    });
  }
</script>

<!--
  The one place cards are wired up (B2). The library, a category's full list and Inicio's carousels
  all render this, so "the same cards as the library" is a fact of the code rather than three copies
  of the same markup that can drift. `layout` only changes how the list is laid out: a wrapping grid
  for a full list, a horizontal scrolling strip for a carousel (B1).
-->
<div class="grid-wrap" class:carousel={layout === "strip"}>
  {#if layout === "strip"}
    <!-- R15: the arrows are the carousel's controls. They are disabled at their end rather than
         hidden, so the strip does not change width when the reader reaches the last tile. -->
    <button
      type="button"
      class="strip-arrow prev"
      aria-label={t("home.carouselPrevious")}
      disabled={atStart}
      onclick={() => scrollTiles(-1)}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
        stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M10 3.5 5.5 8l4.5 4.5" />
      </svg>
    </button>
  {/if}

  <ul class="grid" class:strip={layout === "strip"} bind:this={strip} onscroll={measure}>
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

  {#if layout === "strip"}
    <button
      type="button"
      class="strip-arrow next"
      aria-label={t("home.carouselNext")}
      disabled={atEnd}
      onclick={() => scrollTiles(1)}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
        stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M6 3.5 10.5 8 6 12.5" />
      </svg>
    </button>
  {/if}
</div>

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
    <!-- R16. Two groups, because the editor changes two different kinds of thing with two different
         saves: the text (title and authors, committed together) and the cover. Five controls stacked
         in one column never said which Save applied to what — that was the "estructura" the user
         asked to improve. -->
    <section class="group">
      <h3 class="group-heading">{t("card.editorText")}</h3>

      <div class="title-field">
        <span>{t(editor.yours ? "card.titleLabelYours" : "card.titleLabel")}</span>
        <!-- R16: the restore control sits to the LEFT of the field it acts on, and it is an icon with
             a hover popover instead of a sentence the width of the dialog. A full-text button gave a
             secondary action the weight of a primary one. -->
        <div class="title-row">
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
          {#if editor.yours}
            <!-- R17: the user's correction moved it to the RIGHT of the field. It is a trailing action
                 on what the field holds, and at the right it reads as one. -->
            <button
              type="button"
              class="restore-icon"
              aria-label={t("card.titleRestore")}
              data-tip={t("card.titleRestore")}
              disabled={saving || coverSaving}
              onclick={() => (restoreOpen = true)}
            >
              <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
                stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M6 4.5 2.5 8 6 11.5" />
                <path d="M2.5 8h7a4 4 0 0 1 0 8H7" />
              </svg>
            </button>
          {/if}
        </div>
      </div>

      {#if titleError !== null}
        <p class="title-error" role="alert">{titleError}</p>
      {/if}

      <div class="title-field">
        <span>{t(author.yours ? "card.authorLabelYours" : "card.authorLabel")}</span>
        {#if authorChips.length > 0}
          <ul class="author-chips">
            {#each authorChips as name (name)}
              <li class="author-chip">
                <span class="author-chip-name">{name}</span>
                <button
                  type="button"
                  class="author-chip-remove"
                  aria-label={t("card.authorRemove", { author: name })}
                  onclick={() => removeAuthor(name)}
                  disabled={saving || coverSaving}
                >
                  ×
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <!-- R16: the field IS the selector. Typing narrows the list; the arrow keys walk it; Enter
             takes the highlighted name, or the typed one when nothing is highlighted; Escape closes
             it. A `<datalist>` was never an option here — WebKitGTK does not draw its popup (L70) —
             so the listbox is ours, with the roles a screen reader expects. -->
        <div class="combobox">
          <input
            type="text"
            bind:value={authorInput}
            placeholder={t("card.authorAddPlaceholder")}
            aria-label={t("card.authorAdd")}
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={suggestionsOpen && choices.length > 0}
            aria-controls="author-choices"
            aria-activedescendant={suggestionsOpen && highlighted >= 0
              ? `author-choice-${highlighted}`
              : undefined}
            onfocus={() => {
              suggestionsOpen = true;
              highlighted = -1;
            }}
            onblur={() => {
              suggestionsOpen = false;
              highlighted = -1;
            }}
            onkeydown={authorKeydown}
          />
          {#if suggestionsOpen && choices.length > 0}
            <div class="choices" id="author-choices" role="listbox" aria-label={t("card.authorAdd")}>
              {#each choices as choice, index (choice)}
                <button
                  type="button"
                  id={`author-choice-${index}`}
                  role="option"
                  aria-selected={index === highlighted}
                  class:highlighted={index === highlighted}
                  onmousedown={(event) => event.preventDefault()}
                  onclick={() => pickAuthor(choice)}
                >{choice}</button>
              {/each}
            </div>
          {/if}
        </div>
        <p class="field-hint">{t("card.authorHint")}</p>
      </div>

      <!-- R18: the pair exists only while there is something to save or to discard. A Save that is
           always there invites a write that changes nothing, and a Cancel that is always there is a
           button with no meaning. Closing the editor is the backdrop or Escape, which the modal
           already offers. -->
      {#if textDirty}
        <div class="confirm-row">
          <button
            type="button"
            class="action secondary"
            onclick={discardTextDraft}
            disabled={saving || coverSaving}
          >
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
              stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M4 4l8 8" />
              <path d="M12 4l-8 8" />
            </svg>
            {t("card.titleCancel")}
          </button>
          <button
            type="button"
            class="action primary"
            onclick={() => void saveTitle()}
            disabled={saving || coverSaving}
          >
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
              stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M3.5 8.5 6.5 11.5 12.5 4.5" />
            </svg>
            {t("card.titleSave")}
          </button>
        </div>
      {/if}
    </section>

    <section class="group">
      <h3 class="group-heading">{t("card.coverEditor")}</h3>

      <!-- R18: the section says WHAT the cover is before saying how to change it. The user's report
           was that the old layout left it unclear whether the cover was a page of the document or a
           file of their own — the preview and the two mechanisms sat together with nothing saying
           which one was in force. Now the preview carries a sentence stating the current source, and
           the two ways to change it are two labelled blocks that cannot be confused. -->
      <div class="cover-layout">
        <div class="cover-preview" aria-label={t("card.coverPreview")}>
          {#if editorCover !== null}
            <img src={editorCover} alt={t("card.coverPreview")} />
          {:else}
            <span>{t("card.cover")}</span>
          {/if}
        </div>

        <div class="cover-controls">
          <p class="cover-current">
            {coverIsImage
              ? t("card.coverCurrentImage")
              : t("card.coverCurrentPage", {
                  page: savedPage.trim() === "" ? "1" : savedPage,
                })}
          </p>
          <p class="field-hint">{t("card.coverSourceHint")}</p>
        </div>
      </div>

      {#if coverError !== null}
        <p class="title-error" role="alert">{coverError}</p>
      {/if}

      <div class="cover-option">
        <h4 class="option-heading">{t("card.coverPageSection")}</h4>
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
        <p class="field-hint">{t("card.coverPageHint")}</p>
        {#if coverDirty}
          <div class="confirm-row">
            <button
              type="button"
              class="action secondary"
              onclick={() => void saveCoverPage()}
              disabled={saving || coverSaving}
            >
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
                stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M3.5 8.5 6.5 11.5 12.5 4.5" />
              </svg>
              {t("card.coverPageSave")}
            </button>
          </div>
        {/if}
      </div>

      <div class="cover-option">
        <h4 class="option-heading">{t("card.coverImageSection")}</h4>
        <div class="confirm-row">
          {#if coverHasImage && !coverIsImage}
            <!-- R19: the image is stored but the page took over. This brings it back without the
                 reader choosing the file again — the half of the request that did not exist before. -->
            <button
              type="button"
              class="action secondary"
              onclick={() => void useStoredImage()}
              disabled={saving || coverSaving}
            >
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
                stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M3.5 8.5 6.5 11.5 12.5 4.5" />
              </svg>
              {t("card.coverUseImage")}
            </button>
          {/if}
          <button
            type="button"
            class="action secondary"
            onclick={() => void chooseCoverImage()}
            disabled={saving || coverSaving}
          >
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
              stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <rect x="2.5" y="3.5" width="11" height="9" rx="1.5" />
              <circle cx="6" cy="7" r="0.9" />
              <path d="M3 11.4 6.4 8l2.4 2.4L10.3 9l2.4 2.4" />
            </svg>
            {t("card.coverChooseImage")}
          </button>
          {#if coverHasImage}
            <button
              type="button"
              class="action danger"
              onclick={() => void removeCoverImage()}
              disabled={saving || coverSaving}
            >
              <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
                stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M3 4.5h10" />
                <path d="M6.5 4.5V3h3v1.5" />
                <path d="M4.5 4.5 5 13h6l.5-8.5" />
              </svg>
              {t("card.coverRemoveImage")}
            </button>
          {/if}
        </div>
      </div>
    </section>
  </Modal>

  <!-- R18: restoring the document's title is destructive, so it asks. The sentence states what is
       lost, including the draft on screen, because the editor closes after it: the field cannot show
       the document's own title without re-reading the listing, and a field showing the discarded one
       would be worse than no field. -->
  {#if restoreOpen}
    <Modal title={t("card.restoreConfirmTitle")} onClose={() => (restoreOpen = false)}>
      <p class="confirm-body">{t("card.restoreConfirmBody")}</p>
      <div class="confirm-row">
        <button
          type="button"
          class="action secondary"
          onclick={() => (restoreOpen = false)}
        >
          {t("card.titleCancel")}
        </button>
        <button type="button" class="action primary" onclick={confirmRestore}>
          {t("card.restoreConfirmAction")}
        </button>
      </div>
    </Modal>
  {/if}
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
    column-gap: 0.6rem;
    row-gap: 0.6rem;
  }

  .grid li {
    display: flex;
  }

  /* B1: a carousel scrolls sideways rather than wrapping. The tiles keep a fixed width so the strip
     has a definite end to scroll to, and `overflow-x: auto` is the scroll itself. */
  /* The strip's frame. A plain block for a grid; for a carousel it becomes the positioning context
     the two arrows hang from, so they float over the strip's edges instead of taking width from it. */
  .grid-wrap.carousel {
    position: relative;
  }

  .strip-arrow {
    position: absolute;
    top: 40%;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.85rem;
    height: 1.85rem;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-full);
    background: rgba(7, 8, 10, 0.78);
    color: var(--fg);
    cursor: pointer;
    backdrop-filter: blur(6px);
    transition: background var(--fast) var(--ease), opacity var(--fast) var(--ease);
  }

  .strip-arrow:hover:not(:disabled) {
    background: var(--surface-2);
  }

  .strip-arrow:disabled {
    opacity: 0;
    pointer-events: none;
  }

  .strip-arrow.prev {
    left: -0.35rem;
  }

  .strip-arrow.next {
    right: -0.35rem;
  }

  .grid.strip {
    display: flex;
    flex-wrap: nowrap;
    overflow-x: auto;
    /* R15: the scrollbar under a carousel was the user's complaint — the arrows are the affordance
       now. The strip still scrolls by pointer, trackpad and keyboard; only the bar is gone. */
    scrollbar-width: none;
    -ms-overflow-style: none;
    /* R17: as soon as one axis is not `visible`, the other computes to `auto` too — so `overflow-x`
       alone silently enabled a vertical scrollbar, which showed for the overflow described in
       BookCard's fixed-height body. The strip scrolls sideways and only sideways, and this says so
       rather than leaving it to a CSS default; the overflow itself was fixed where it happened. */
    overflow-y: hidden;
    gap: 0.6rem;
    padding-bottom: 0.35rem;
  }

  /* Every control in this component — the title field, the cover page, the author field and the author
     list — is one shape: the reference's 2 rem control, one surface above the card it sits on. Stated
     once because the four of them drifting apart is what made the editor read as four forms. */
  /* R17: every field in the editor — the title, the author combobox, the page — is one shape.
     This rule was destroyed by a CSS cleanup that matched the middle of an `input, select` block, and
     the editor fell back to the browser's own 21-pixel fields: the user's screenshot is the result.
     It is re-stated as its own rule so a future `select` cleanup cannot take it down with it. */
  input {
    width: 100%;
    height: 2.25rem;
    padding: 0 0.6rem;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background-color: var(--surface-1);
    color: var(--fg);
    font: inherit;
    font-size: var(--text-sm);
    box-sizing: border-box;
  }

  input::placeholder {
    color: var(--fg-subtle);
  }

  .grid.strip::-webkit-scrollbar {
    display: none;
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
    font-size: var(--text-xs);
    color: var(--fg-subtle);
  }

  /* R40: the field and the list beside it. The field takes the room, because typing stays the primary
     gesture and the list is the shortcut for a name the library already knows. */


  /* R42/U30: the chips, one per author, wrapping in the reader's order. They sit above the field
     because they are the VALUE — the field below only says how the next one gets in. */
  .author-chips {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }

  .author-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0.2rem 0.2rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: var(--r-full);
    background: var(--surface-2);
    font-size: var(--text-xs);
    color: var(--fg);
  }

  .author-chip-remove {
    padding: 0 0.35rem;
    border: none;
    border-radius: var(--r-full);
    background: none;
    color: var(--fg-muted);
    font: inherit;
    font-size: 0.95rem;
    line-height: 1;
    cursor: pointer;
  }

  .author-chip-remove:hover:not(:disabled) {
    background: var(--surface-3);
    color: var(--fg);
  }

  .author-chip-remove:disabled {
    color: var(--fg-subtle);
    cursor: default;
  }





  .title-error {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--danger);
  }

  /* R32: the cover half of the one editor. The heading separates it from the title above without a
     second modal, and it names what the controls below act on. */


  /* R32: the cover editor's page field and its hint, sharing the title field's chrome. */
  .cover-field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: var(--text-xs);
    color: var(--fg-subtle);
  }















  /* The restore action restores rather than destroys, so it reads as a secondary choice: it is
     pushed to the left, away from the cancel/save pair. */


  /* ---- R16: the editor's own structure ---------------------------------------------------- */

  /* A group is one kind of change with its own save, and the rule between them says so. */
  .group {
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }

  .group + .group {
    margin-top: 0.35rem;
    padding-top: 0.85rem;
    border-top: 1px solid var(--line);
  }

  .group-heading {
    margin: 0;
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
    text-transform: uppercase;
    letter-spacing: var(--tracking-wide);
    color: var(--fg-subtle);
  }

  /* The restore control and its field are one row, with the control first: it acts on what is beside
     it, and it is an icon, so the field keeps its width. */
  .title-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .title-row input {
    flex: 1 1 auto;
    min-width: 0;
  }

  .restore-icon {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--surface-1);
    color: var(--fg-muted);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .restore-icon:hover:not(:disabled),
  .restore-icon:focus-visible {
    background: var(--surface-2);
    color: var(--fg);
  }

  /* The popover is CSS, not a library: it says why the icon exists on hover and on keyboard focus,
     which is the two ways a person can be unsure about an icon. */
  .restore-icon::after {
    content: attr(data-tip);
    position: absolute;
    bottom: calc(100% + 0.35rem);
    /* R18: anchored to the BUTTON's right edge, not centred on it. The control is the last thing in
       the row, so a centred popover stuck out past the dialog and — because a dialog that scrolls
       vertically also scrolls horizontally — produced a horizontal scrollbar across the whole editor.
       Growing leftwards keeps the sentence inside the modal at any width. */
    right: 0;
    z-index: 4;
    padding: 0.2rem 0.45rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-sm);
    background: var(--surface-3);
    color: var(--fg);
    font-size: var(--text-xs);
    white-space: nowrap;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--fast) var(--ease);
  }

  .restore-icon:hover::after,
  .restore-icon:focus-visible::after {
    opacity: 1;
  }

  .combobox {
    position: relative;
    width: 100%;
  }

  .choices {
    position: absolute;
    top: calc(100% + 0.25rem);
    right: 0;
    left: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    max-height: 9rem;
    overflow-y: auto;
    padding: 0.2rem;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-md);
    background: var(--surface-2);
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.55);
  }

  .choices button {
    padding: 0.35rem 0.5rem;
    border: 0;
    border-radius: var(--r-xs);
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  .choices button:hover,
  .choices button.highlighted {
    background: var(--surface-3);
  }

  .field-hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--fg-subtle);
  }

  .cover-layout {
    display: flex;
    align-items: flex-start;
    gap: 0.8rem;
  }

  /* The preview is the same 9/11 rectangle the card uses, so "the cover" means the same shape in both
     places. */
  .cover-preview {
    display: grid;
    flex: none;
    place-items: center;
    width: 4.4rem;
    aspect-ratio: 9 / 11;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    background: var(--surface-1);
    color: var(--fg-subtle);
    font-size: var(--text-xs);
    text-align: center;
  }

  .cover-preview img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* R18: the sentence that says what the cover IS, above the two ways to change it. */
  .cover-current {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--fg);
  }

  /* One block per mechanism, divided by a rule: a page of the document, or an image of the reader's.
     Two labels are what stop the old ambiguity — the preview used to sit beside both with nothing
     saying which one was in force. */
  .cover-option {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-top: 0.65rem;
    border-top: 1px solid var(--line);
  }

  .option-heading {
    margin: 0;
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
  }

  .cover-option .confirm-row {
    justify-content: flex-start;
  }

  .confirm-body {
    margin: 0;
    color: var(--fg-muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .cover-controls {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    min-width: 0;
    gap: 0.4rem;
  }

  /* Every button in the editor is one shape: an action with a drawing and a word, so the two saves
     and the three cover actions do not have to be read to be told apart. The primary is the one loud
     control in the dialog. */
  .confirm-row {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.4rem;
  }

  .action {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.4rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--surface-1);
    color: var(--fg);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--fast) var(--ease), border-color var(--fast) var(--ease);
  }

  .action:hover:not(:disabled) {
    background: var(--surface-2);
    border-color: var(--line-strong);
  }

  .action:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .action.primary {
    border-color: transparent;
    background: var(--fg);
    color: var(--fg-on-loud);
    font-weight: var(--weight-medium);
  }

  .action.primary:hover:not(:disabled) {
    background: #ffffff;
    border-color: transparent;
  }

  .action.danger {
    color: var(--danger);
  }

  .action.danger:hover:not(:disabled) {
    border-color: rgba(255, 99, 99, 0.4);
    background: rgba(255, 99, 99, 0.12);
  }

</style>
