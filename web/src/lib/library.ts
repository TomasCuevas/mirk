/**
 * The screen's decisions, as plain functions with no DOM and no framework.
 *
 * Everything here is a value the interface renders: a reading position turned into a sentence, an
 * empty library turned into a message, a provider turned into a capability statement. Keeping it
 * out of the Svelte component means it can be tested without mounting anything — and, more
 * importantly, that the honesty rules have one home instead of living in template markup.
 *
 * # The rule this module exists to enforce
 *
 * Progress is stated **with its provenance, in words**. Never as a bare percentage. A
 * precise-looking "40%" that really means "this file was closed four months ago" is a lie, which
 * is exactly what the store's separate `source` and `confidence` columns exist to prevent. The
 * position sentence therefore always names where the number came from, and it never invents a
 * denominator: no total means no "of N" and no percentage.
 *
 * # Zero-based pages
 *
 * The store is zero-based (S4 in `oct-store`); people count pages from one. The conversion happens
 * here, at the only boundary that renders a page to a person.
 */

import type { Translate } from "./i18n";

/** A reading position with its provenance, as returned by the `list_library` command. */
export interface Progress {
  /** Zero-based page index, exactly as stored. */
  page: number;
  /** Total pages when the position's own source knew one; never invented. */
  total: number | null;
  /** `"native"`, `"manual"`, or an external provider id such as `"evince-gvfs"`. */
  source: string;
  /** `"exact"`, `"close-time"`, `"manual"` or `"unknown"`. */
  confidence: string;
  /**
   * Unix seconds when the position was recorded — when the APP wrote the row. It is a real fact and it is
   * deliberately not the answer to "when did you read this": the launch sweep writes a whole library at
   * once, so this says when we looked.
   */
  recordedAt: number | null;
  /**
   * Unix seconds when a source that OBSERVED the reading says it happened, or null for "no source said".
   *
   * Null is not "long ago". It is the honest answer for a document no source ever recorded, and it must
   * stay distinguishable from a time — which is why the category rule and the carousels' order read this
   * field and not `recordedAt`.
   */
  readAt: number | null;
}

/** One document in the library view. */
export interface LibraryEntry {
  id: number;
  path: string;
  /**
   * The title to display, **already resolved by the core** (R31).
   *
   * It is the reader's title when they have set one, otherwise the document's own — the embedded
   * one, or the file's own name. Because the core resolves it, the interface never decides whose
   * title it is rendering and no card can fall back to a path. A blank value never arrives here.
   */
  title: string;
  /**
   * The reader's own title, or `null` when they have not set one (`userTitle` from the Rust view).
   *
   * This is what lets a card say "this title is yours": the reader's title is a separate fact, so
   * the interface can tell it apart from the document's own rather than having to guess. The backend
   * always sends it — `null` means the reader has not set one — so a listing that forgets it must not
   * typecheck.
   */
  userTitle: string | null;
  /**
   * The author to display, **already resolved by the core**.
   *
   * It is the reader's author when they have set one, otherwise the document's own — and `null`
   * when neither exists. Because the core resolves it, the interface never decides whose author it
   * is rendering and the card keeps showing the author the way it always has.
   */
  author: string | null;
  /**
   * The reader's own author, or `null` when they have not set one (`userAuthor` from the Rust view).
   *
   * This is what tells the editor whether the author on screen is the reader's, and what the
   * suggestions list draws on beside the resolved authors. The backend always sends it — `null`
   * means the reader has not set one — so a listing that forgets it must not typecheck.
   */
  userAuthor: string | null;
  /** Page count from the scanner; null when it was never established. */
  pages: number | null;
  progress: Progress | null;
  /**
   * The page the reader chose for the cover, zero-based (`coverPage` from the Rust view), or `null`
   * for the first page (R32). Optional on the type so a listing shaped before R32 still compiles.
   */
  coverPage?: number | null;
  /**
   * The file name of an image the reader supplied, inside the covers directory, or `null` (R32).
   * Only a file name travels here: the application owns the copy inside its own directory.
   */
  coverImage?: string | null;
}

/** What the `open_containing_folder` command returns when the folder is opened. */
export interface OpenedFolder {
  folder: string;
  viewer: string;
}

/** What the `open_document` command returns when the document is opened. */
export interface OpenedDocument {
  /** The document that was opened, by path. */
  path: string;
  /** The program that opened it: the desktop's default handler, or a page-capable viewer. */
  viewer: string;
  /**
   * The zero-based page the document was resumed at, or `null` when it was opened at its beginning.
   *
   * `null` is not "page zero": it is "no source ever reported a position for this document", which
   * is why the two cases are different answers rather than the same number.
   */
  page: number | null;
}

/**
 * What a card's "open the document" action produced.
 *
 * The same shape the folder action has, and for the same reason: the card renders no outcome — the
 * two sentences that used to draw it were removed at the user's request (L27, L30) — so the decision
 * is kept in a plain value that both branches can be pinned on without a webview.
 */
export type DocumentActionOutcome =
  | { kind: "opened"; path: string; viewer: string; page: number | null }
  | { kind: "failed"; message: string };

/**
 * What a card's "open containing folder" action produced.
 *
 * The outcome is a real decision even though no screen draws it: opening the folder either named the
 * folder and the viewer that took it, or it did not happen at all. It is a plain value rather than
 * component state, so both branches can be pinned without a webview — which matters here because the
 * two sentences that used to render it were removed at the user's request (L27, L30), and this rule
 * is what keeps the behaviour checked now that nothing draws it.
 */
export type FolderActionOutcome =
  | { kind: "opened"; folder: string; viewer: string }
  | { kind: "failed"; message: string };

/**
 * Decides the folder action's outcome from the command's settled answer, and nothing else.
 *
 * A fulfilled `open_containing_folder` is the folder that was opened and the viewer that opened it; a
 * rejection is a failure whose message the caller reports to the console — the same place the window
 * actions report failures that have no user state to carry them. A `null` or an error is never
 * dressed up as an opened folder.
 */
export function folderActionOutcome(
  settled: PromiseSettledResult<OpenedFolder>,
): FolderActionOutcome {
  if (settled.status === "fulfilled") {
    return { kind: "opened", folder: settled.value.folder, viewer: settled.value.viewer };
  }
  return { kind: "failed", message: String(settled.reason) };
}

/**
 * Decides the open-document action's outcome from the command's settled answer, and nothing else.
 *
 * A fulfilled `open_document` is the document that was opened, the program that opened it and the
 * page it was resumed at; a rejection is a failure whose message the caller reports to the console —
 * the same place the folder action reports a failure that has no user state to carry it. A `null` or
 * an error is never dressed up as an opened document.
 */
export function documentActionOutcome(
  settled: PromiseSettledResult<OpenedDocument>,
): DocumentActionOutcome {
  if (settled.status === "fulfilled") {
    return {
      kind: "opened",
      path: settled.value.path,
      viewer: settled.value.viewer,
      page: settled.value.page,
    };
  }
  return { kind: "failed", message: String(settled.reason) };
}

/** A progress provider and whether this machine can use it. */
export interface Provider {
  id: string;
  capability: string;
  availability: string;
}

/** A document the scan counted and persisted. */
export interface CountedDocument {
  path: string;
  pages: number;
  title: string | null;
  author: string | null;
}

/** A path the scan could not turn into a counted document, and why. */
export interface ScanFailure {
  path: string;
  reason: string;
}

/** What a scan of one folder returned. */
export interface ScanReport {
  root: string;
  discovered: string[];
  counted: CountedDocument[];
  skipped: string[];
  failures: ScanFailure[];
}

/**
 * The colours the progress fill travels through, at the fractions that anchor them (R39, L68).
 *
 * The four colours the user named are the **landmarks** of one continuous ramp, and their positions are a
 * reading decision, not an even split: green is reached at **70 %**, because a document two thirds read
 * is nearly finished and a reader who has got that far has stopped wanting to look at amber. The user's
 * own words, with their screenshot: *"siento que el verde ya deberia de comenzar desde el 70%, ya que
 * elementos que van por encima del 70% se ven muy amarillos"* — at 76 % the fill was olive, and it is
 * now plainly green.
 *
 * There is a fifth stop beyond green on purpose: a ramp that held one green from 70 % to 100 % would
 * stop moving exactly where the last stretch of a long book lives, so the green keeps deepening instead
 * of flattening. The width already states how much is left; the colour no longer stops saying it.
 *
 * They are deliberately dark for their hue. The bar they are painted on is pale (#e8ebef) and the card
 * is white, so a light yellow would be a bar that disappears at exactly the moment it means "halfway".
 * `progressColor` interpolates in a perceptual space, which keeps the intermediate colours as dark as
 * the stops instead of muddying them the way naive RGB blending does.
 */
const PROGRESS_STOPS: readonly { at: number; hex: string }[] = [
  { at: 0, hex: "#b3261e" },
  { at: 0.28, hex: "#c2410c" },
  { at: 0.5, hex: "#a16207" },
  { at: 0.7, hex: "#2e7d32" },
  { at: 1, hex: "#1e5b22" },
];

/** A colour in OKLab: perceptual lightness, then the two opponent axes. */
interface Oklab {
  readonly l: number;
  readonly a: number;
  readonly b: number;
}

/**
 * The colour of the progress fill for a fraction read (R39).
 *
 * **One continuous ramp, not four bands.** The user's own correction: *"no quiero que sean colores
 * fijos, quiero que se vayan transformando poco a poco a medida que el porcentaje avanza, no que
 * cambie bruscamente"*. The four colours they named are the stops; every fraction between two stops
 * gets its own colour, so a card at 41 % and a card at 42 % differ by a shade nobody would notice
 * individually and everybody would notice as a jump.
 *
 * **Interpolated in OKLab, not in sRGB.** Blending red and green channel-by-channel in sRGB runs
 * through a muddy olive; OKLab is close enough to how the eye works that the ramp keeps its chroma and
 * its lightness. The output is plain `rgb()`, so no engine support beyond CSS Color 3 is needed — the
 * perceptual work happens here, where it can be tested, not in the browser.
 *
 * The input is clamped to `0..1` and a value that cannot be read (`NaN`) takes the floor, for the same
 * reason the band did: a colour is a claim, and one invented from a broken number would be a lie. The
 * floor is red at 0 %, which is also what an empty fill draws — nothing.
 */
export function progressColor(fraction: number): string {
  const clamped = Number.isNaN(fraction) ? 0 : Math.min(1, Math.max(0, fraction));

  // The segment this fraction falls in. `stops.length - 1` segments, so the last one is inclusive of
  // the top: `clamped === 1` belongs to the final segment rather than past the end of the array.
  let index = 0;
  while (index < PROGRESS_STOPS.length - 2 && clamped > PROGRESS_STOPS[index + 1].at) {
    index += 1;
  }
  const from = PROGRESS_STOPS[index];
  const to = PROGRESS_STOPS[index + 1];
  const span = to.at - from.at;
  const position = span === 0 ? 0 : (clamped - from.at) / span;

  const start = hexToOklab(from.hex);
  const end = hexToOklab(to.hex);
  return oklabToRgb({
    l: start.l + (end.l - start.l) * position,
    a: start.a + (end.a - start.a) * position,
    b: start.b + (end.b - start.b) * position,
  });
}

/** `#rrggbb` to OKLab, through linear sRGB (Ottosson's published matrices). */
function hexToOklab(hex: string): Oklab {
  const value = hex.replace("#", "");
  const r = srgbToLinear(Number.parseInt(value.slice(0, 2), 16) / 255);
  const g = srgbToLinear(Number.parseInt(value.slice(2, 4), 16) / 255);
  const b = srgbToLinear(Number.parseInt(value.slice(4, 6), 16) / 255);

  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);

  return {
    l: 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    a: 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    b: 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  };
}

/** OKLab back to an `rgb()` string a stylesheet can use. */
function oklabToRgb(colour: Oklab): string {
  const l = Math.pow(colour.l + 0.3963377774 * colour.a + 0.2158037573 * colour.b, 3);
  const m = Math.pow(colour.l - 0.1055613458 * colour.a - 0.0638541728 * colour.b, 3);
  const s = Math.pow(colour.l - 0.0894841775 * colour.a - 1.291485548 * colour.b, 3);

  const r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const b = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;

  return `rgb(${toByte(r)}, ${toByte(g)}, ${toByte(b)})`;
}

/** Linear sRGB to the sRGB transfer function the display expects. */
function linearToSrgb(channel: number): number {
  const clamped = Math.min(1, Math.max(0, channel));
  return clamped <= 0.0031308 ? 12.92 * clamped : 1.055 * Math.pow(clamped, 1 / 2.4) - 0.055;
}

/** The sRGB transfer function to linear light. */
function srgbToLinear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : Math.pow((channel + 0.055) / 1.055, 2.4);
}

/** A linear channel to the 0..255 byte an `rgb()` string carries. */
function toByte(linear: number): number {
  return Math.round(linearToSrgb(linear) * 255);
}

/** What the card draws and says about reading progress (R7). */
export interface ProgressView {
  /** The bar's fill, `0..1`. `0` when nothing is recorded, which is what the card was asked to show. */
  readonly fraction: number;
  /** The text over the bar: the whole percentage, or the page when no total is known. */
  readonly marker: string;
  /** Where the marker sits along the bar, `0..1` — the fraction itself, so the number states the
   *  fraction rather than merely sitting next to it. */
  readonly markerAt: number;
  /**
   * The fill's colour, as a CSS `rgb()` (R39). It is a continuous ramp over the fraction the width
   * comes from, so a long bar's colour and its length always tell the same story.
   */
  readonly color: string;
  /** The provenance in words, or `null` when nothing is recorded — the one case with no sentence. */
  readonly provenance: string | null;
}

/**
 * The card's progress: a fraction to draw, a number to place on it, and the provenance in words.
 *
 * Three cases, and the difference between the last two is the point:
 *
 * - **Recorded with a total** — the bar's fill and the marker's position are the same number, taken
 *   from `progressFraction`, so the card and the progress sort can never disagree about a fraction.
 * - **Recorded without a total** — the reader is demonstrably *somewhere* in the document, so the
 *   marker states the page and the bar stays empty. A percentage here would be invented, and an
 *   invented percentage is worse than none.
 * - **Nothing recorded** — `0 %`, the user's decision (L23), and no sentence at all: the empty bar
 *   says it, and repeating it in words is the thing they asked to be removed.
 */
export function describeProgressView(entry: LibraryEntry, t: Translate): ProgressView {
  const progress = entry.progress;

  if (progress === null) {
    return {
      fraction: 0,
      marker: t("card.progressPercent", { percent: 0 }),
      markerAt: 0,
      color: progressColor(0),
      provenance: null,
    };
  }

  const provenance = describeProvenance(progress, t);
  const fraction = progressFraction(entry);

  if (fraction === null) {
    return {
      fraction: 0,
      marker: t("progress.position", { page: progress.page + 1 }),
      markerAt: 0,
      color: progressColor(0),
      provenance,
    };
  }

  return {
    fraction,
    marker: t("card.progressPercent", { percent: Math.round(fraction * 100) }),
    markerAt: fraction,
    color: progressColor(fraction),
    provenance,
  };
}

/**
 * The provenance half of the sentence, derived from `source` and `confidence` **separately**.
 *
 * The two columns mean different things: `source` is which mechanism produced the number, and
 * `confidence` is how far it should be trusted. Flattening them would lose the distinction the
 * screen exists to show, so an "unknown" confidence wins over the source, a manual entry wins over
 * an automatic one, and a close-time viewer says so instead of claiming to be exact.
 */
export function describeProvenance(progress: Progress, t: Translate): string {
  if (progress.confidence === "unknown") {
    return t("progress.unknown");
  }
  if (progress.source === "manual") {
    return t("progress.manual");
  }

  const viewer = sourceLabel(progress.source, t);
  if (progress.confidence === "close-time") {
    return t("progress.closed", { viewer });
  }
  return t("progress.by", { viewer });
}

/** The message an empty library shows, or `null` once there is something to list. */
export function describeLibraryMessage(
  entries: readonly LibraryEntry[],
  t: Translate,
): string | null {
  return entries.length === 0 ? t("library.empty") : null;
}

/**
 * What a listing screen shows, once the listing and the work behind it are both taken into account.
 *
 * The three values are deliberately distinct, because "still loading", "loaded and genuinely empty"
 * and "loaded with documents" are three different facts and conflating two of them is the defect this
 * function exists to prevent.
 */
export type LibraryView = "loading" | "empty" | "content";

/**
 * Decides what a listing screen shows (L47).
 *
 * - **content** — the listing holds something, and that WINS over a loader: a re-read of a listing
 *   that is already on screen (the fifteen-second tick, or a title saved on another screen) must not
 *   cover documents with a spinner or flash an empty grid.
 * - **loading** — the listing is empty and a read the reader caused is in flight (the first load, a
 *   scan, or the reload a scan or a folder removal triggers). An empty listing may only be shown AS
 *   empty once nothing is in flight; before that, the screen would be claiming there is nothing to
 *   find while it is still looking — the false "no watched folder" note the user saw after
 *   re-adding a folder.
 * - **empty** — the listing is empty and nothing is in flight. Only now is "there is nothing" an
 *   honest thing to say.
 *
 * `inFlight` deliberately excludes the silent fifteen-second re-read: that path raises no flag, which
 * is what keeps it invisible on a screen that already has content and is why it is a separate path
 * from the first load.
 *
 * The decision is a function rather than a condition in markup so it can be pinned by a test without
 * rendering anything, and so Inicio, Biblioteca and a category's own list cannot drift apart on it.
 */
export function libraryView(
  entries: readonly LibraryEntry[],
  inFlight: boolean,
): LibraryView {
  if (entries.length > 0) {
    return "content";
  }
  return inFlight ? "loading" : "empty";
}

/**
 * A provider's capability and availability, in words.
 *
 * A provider that is missing names **why**, rather than showing a disabled control with no
 * explanation: `detect()` only reports that it is absent, so the reason comes from the catalogue
 * for that provider id.
 */
export function describeProvider(provider: Provider, t: Translate): string {
  const viewer = sourceLabel(provider.id, t);

  if (provider.availability === "installed") {
    return t("provider.installed", {
      viewer,
      capability: capabilityLabel(provider.capability, t),
    });
  }
  if (provider.availability === "unsupported") {
    return t("provider.unsupported", { viewer });
  }
  return t("provider.missing", {
    viewer,
    capability: capabilityLabel(provider.capability, t),
    reason: providerReason(provider.id, t),
  });
}

/** The message shown when the platform exposes no providers at all, or `null` when it does. */

/**
 * What to say about the provider registry.
 *
 * `null` means the registry has not arrived yet, which is **not** the same as "there are none": a
 * screen that says "no providers are available" before the answer arrives is claiming something it
 * does not know, which is the failure mode this project refuses everywhere else.
 */
export function describeProviderRegistry(
  providers: readonly Provider[] | null,
  t: Translate,
): string | null {
  if (providers === null) {
    return t("provider.loading");
  }
  return providers.length === 0 ? t("provider.none") : null;
}

/** What a scan did: how many PDFs it found, how many it counted, and where. */
export function describeScanSummary(report: ScanReport, t: Translate): string {
  if (report.discovered.length === 0) {
    return t("scan.none", { root: report.root });
  }
  return t("scan.summary", {
    counted: report.counted.length,
    discovered: report.discovered.length,
    root: report.root,
  });
}

/**
 * What the title editor shows for one document (R31).
 *
 * The editor is where provenance is stated, and it says it twice without a word of explanation: the
 * field's label names WHOSE title is being edited, and the restore control exists exactly when there is
 * a reader's title to go back from — restoring something that was never set is an action without a
 * meaning. The user asked for the note on the card to go (on a title they had just typed themselves it
 * was noise), so that information lives here, at the moment it is actually needed.
 *
 * It is a function rather than a condition inside the markup because it is a RULE: it has to be pinned
 * by a test, and a test cannot click a modal.
 */
export function titleEditor(entry: LibraryEntry): { value: string; yours: boolean } {
  // Blank is not a title here for the same reason it is not one in `Document::display_title`, which
  // ignores a blank reader's title: the store refuses to write one, so this only matters if a value
  // arrives from somewhere less careful — and then the interface must not offer to "restore" it.
  const yours = entry.userTitle != null && entry.userTitle.trim() !== "";
  return { value: displayTitle(entry), yours };
}

/**
 * What the author editor shows for one document.
 *
 * The twin of [`titleEditor`], with one deliberate difference at the SAVE boundary rather than here:
 * the title refuses a blank because a document must have a title, while an EMPTY author field means
 * *no author of my own* and is a legitimate action that falls back to the document's own author. The
 * field is prefilled with the author already on screen — the reader's when they set one, otherwise
 * the document's own — and `yours` says whether that value is the reader's, so the field's label can
 * state whose author it is instead of the interface having to guess.
 */
export function authorEditor(entry: LibraryEntry): { value: string; yours: boolean } {
  const yours = entry.userAuthor != null && entry.userAuthor.trim() !== "";
  return { value: entry.author ?? "", yours };
}

/**
 * The authors already present in the library, for the editor's native suggestions.
 *
 * The reader should not have to retype a name and risk two spellings of the same person, so the
 * listing they already hold is the source: both the authors the documents carry and the ones readers
 * set are offered. It is derived here rather than fetched with a command, so there is no second
 * source of truth and no new round trip.
 *
 * **Deduplication is by the trimmed value.** Two entries whose authors differ only by surrounding
 * spaces are one person and must appear once; the first spelling wins and the surrounding spaces are
 * never part of the name. The first-seen order is preserved rather than sorted, so the list is a
 * function of the data alone and not of the machine's locale (the same reason the carousels' tiebreak
 * avoids `localeCompare`).
 */
export function libraryAuthors(entries: readonly LibraryEntry[]): string[] {
  const seen = new Set<string>();
  const authors: string[] = [];
  for (const entry of entries) {
    for (const candidate of [entry.author, entry.userAuthor]) {
      const trimmed = candidate?.trim() ?? "";
      if (trimmed !== "" && !seen.has(trimmed)) {
        seen.add(trimmed);
        authors.push(trimmed);
      }
    }
  }
  return authors;
}

/**
 * The title a card shows (R31).
 *
 * The core already resolved it: `entry.title` is the reader's title when they set one, otherwise the
 * document's own — the embedded one, or the file's own name. This helper exists so the card, the
 * filter and the sort all read that one value, and none of them can reach for `path` to decide.
 * `entry.userTitle` is the separate fact that says whether this title is the reader's.
 */
export function displayTitle(entry: LibraryEntry): string {
  return entry.title;
}

/**
 * The entry the document editor must show, from the listing the interface now holds (R32, corrected).
 *
 * The editor is seeded from the entry it was opened with, and that entry is a SNAPSHOT: everything
 * the store changes afterwards — a title, a cover page, an image — leaves it behind. Showing the
 * snapshot is how the reader's own title seemed to vanish after a cover action, and how the page they
 * had chosen came back empty when the editor was reopened. Looking the document up by id in the
 * current listing is the ONE place both facts are made current at once, so the title and the cover
 * cannot drift apart.
 *
 * The opened entry is the fallback, not an error: a listing that no longer names the document must not
 * be able to close an editor the reader is using, so the editor keeps the entry it already has.
 */
export function editorEntry(
  entries: readonly LibraryEntry[],
  opened: LibraryEntry,
): LibraryEntry {
  return entries.find((entry) => entry.id === opened.id) ?? opened;
}

// --- Covers (R32) ----------------------------------------------------------------------------
//
// A card's cover is rendered by the application on demand and cached beside the database. The
// interface's whole job is to ask for it and to keep the placeholder when the answer is "none" —
// never a broken image, never a stand-in that pretends to be a cover.

/** The shape of `tauri.ts`'s `invoke`, so a test can hand a card a fake instead of a webview. */
export type CoverInvoke = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

/**
 * The image source a card may display, or `null` when the placeholder must stand.
 *
 * Only a non-empty string is an image. This is the single place the never-a-broken-image rule is
 * decided, so a card cannot end up with `src=""` — which a browser renders as a request for the
 * page itself — or with `src="null"`, the string a careless template produces from a null answer.
 */
export function coverSource(cover: unknown): string | null {
  return typeof cover === "string" && cover !== "" ? cover : null;
}

/**
 * A document's cover, or `null` when it has none (R32).
 *
 * A rejected request counts as "no cover" too: the command already turns an unreadable document
 * into a null rather than an error, and a cover is not worth a failure message on a card. The reason
 * is reported by the Rust side, where this application reports what it cannot do.
 */
export async function fetchCover(
  entry: LibraryEntry,
  invoke: CoverInvoke,
): Promise<string | null> {
  try {
    return coverSource(await invoke("document_cover", { id: entry.id }));
  } catch {
    return null;
  }
}

/**
 * What the cover editor holds for one document (R32).
 *
 * `page` is the one-based number the field shows, and `hasImage` says whether the "remove the image"
 * control has anything to act on. It is a function rather than a condition inside the markup for the
 * same reason `titleEditor` is: a rule that nothing checks drifts, and a test cannot click a modal.
 */
export function coverEditor(entry: LibraryEntry): { page: string; hasImage: boolean } {
  // A blank image name is not an image, the same way a blank title is not a title: the store refuses
  // to write one, so this only matters for a value that arrived from somewhere less careful.
  const hasImage = entry.coverImage != null && entry.coverImage.trim() !== "";
  return { page: coverPageInput(entry), hasImage };
}

/**
 * The one-based page the cover editor shows for a document (R32).
 *
 * The store keeps pages zero-based and `null` means the first page, so the field shows `1` for a
 * document nobody has chosen a page for. The conversion lives here, at the only boundary that shows a
 * page to a person, exactly as `progress.position` converts a reading position.
 */
export function coverPageInput(entry: LibraryEntry): string {
  return String((entry.coverPage ?? 0) + 1);
}

/** What the cover editor's page field means. */
export type CoverPageChoice = { ok: true; page: number | null } | { ok: false };

/**
 * The zero-based page to store for what the reader typed (R32).
 *
 * Empty is the plain default — the first page — and stores `null`, the value the one cover rule treats
 * as the first page. A positive whole number is converted to zero-based at this boundary. Anything
 * else is refused rather than guessed at: a page is a number, and inventing one from `"abc"` would
 * write a choice the reader never made.
 */
export function coverPageFromInput(input: unknown): CoverPageChoice {
  // The value comes from a DOM field, so it is normalised here rather than assumed: a
  // `<input type="number">` can hand back a number, an emptied field can hand back an empty string,
  // `null` or `undefined`, and all of those must mean the same thing to this rule.
  const trimmed = input === null || input === undefined ? "" : String(input).trim();
  if (trimmed === "") {
    return { ok: true, page: null };
  }
  if (!/^\d+$/.test(trimmed)) {
    return { ok: false };
  }
  const oneBased = Number(trimmed);
  if (oneBased < 1) {
    return { ok: false };
  }
  return { ok: true, page: oneBased - 1 };
}

// --- The editor's outcome (R31/R32) ---------------------------------------------------------

/**
 * What a successful commit in the document editor changed (R31/R32).
 *
 * `"title"` is the title's own Save or Restore, and `"cover"` is any of the cover's actions: choosing
 * a page, choosing an image, or removing one.
 */
export type EditorCommit = "title" | "cover";

/**
 * Whether the document editor closes once a commit succeeds (R32, corrected).
 *
 * The editor edits TWO things — the title (R31) and the cover (R32) — but only the title is decided by
 * the editor's own pair of controls: Save writes the draft the reader typed and Cancel throws it away,
 * and either ends the edit. A cover commit is not that. It decides the cover and NOTHING ELSE, so it
 * must leave the editor open: closing is exactly how an undecided title draft dies, and a cover action
 * that quietly eats what somebody was writing is the defect this rule exists to prevent. The draft
 * survives until the reader presses one of the two controls that mean "this is my title".
 *
 * It is a function rather than a branch inside an event handler for the same reason `titleEditor` is:
 * a rule nothing checks drifts, and the component test environment cannot type into a field, so this
 * is the one place the rule can be pinned.
 */
export function editorClosesAfterCommit(commit: EditorCommit): boolean {
  return commit === "title";
}

/**
 * Whether a successful commit asks the owner of the listing to re-read it (R32, corrected).
 *
 * A commit changes a fact the listing holds, and the interface does not re-read that listing by
 * itself: the owner does, when it is told. The title path always told it, which is why a saved title
 * appeared everywhere at once; the cover path did not, so the listing stayed as it was and reopening
 * the editor seeded its page field from the stale entry — an empty page over a cover the store had
 * already changed. Both halves of the one editor must refresh, so the answer does not depend on which
 * half changed, and a rule that nothing checks does not drift.
 *
 * Like `editorClosesAfterCommit`, it is a function rather than a call inside an event handler because
 * the component test environment cannot click, and this is the one place the rule can be pinned.
 */
export function editorRefreshesLibrary(commit: EditorCommit): boolean {
  return commit === "title" || commit === "cover";
}

// --- Filtering and sorting ------------------------------------------------------------------
//
// Both run over the listing the screen already holds. The decision is recorded in L19 with its
// own expiry: 42 documents — or a few thousand — do not need SQL, and the day a library is large
// enough for that to change is the day the store grows instead.

/**
 * Whether a document matches a free-text query.
 *
 * Case-insensitive, and it searches both the title a reader sees and the path, because a reader
 * looking for a document may remember either. An empty or whitespace-only query matches
 * everything, so clearing the box restores the whole listing.
 */
export function matchesQuery(entry: LibraryEntry, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") {
    return true;
  }
  return (
    displayTitle(entry).toLowerCase().includes(needle) ||
    entry.path.toLowerCase().includes(needle)
  );
}

/** The documents matching `query`, in the order they arrived. */
export function filterEntries(
  entries: readonly LibraryEntry[],
  query: string,
): LibraryEntry[] {
  return entries.filter((entry) => matchesQuery(entry, query));
}

/** The orders the screen offers. */
export type SortKey = "title" | "pages" | "progress";

/** Every sort key, in the order the screen lists them. */
export const sortKeys: readonly SortKey[] = ["title", "pages", "progress"];

/**
 * How far through a document the reader is, as pages read divided by the total, or `null` when
 * that cannot be known.
 *
 * It counts pages **read**, not the zero-based index stored: the card says "p. 12 of 80" for page
 * `11`, so the fraction is `12 / 80`. Keeping the two in step matters — a sort that disagreed with
 * the sentence above it would be the same class of lie the provenance columns exist to prevent.
 *
 * **This deliberately differs from `StoredProgress::fraction()` in the domain, which divides
 * `page / total`.** The domain answers "how many pages are behind the reader"; this answers "which
 * page is the reader on", which is the number the card prints. Aligning them would make the sort
 * disagree with the label beside it, so the divergence is the point — do not "fix" it without
 * changing the sentence too.
 *
 * The result **is** clamped: a position beyond the stored total means finished (`1.0`), matching the
 * domain, rather than a fraction above one that would outrank a completed document.
 *
 * `null` means unknown, and it covers all three ways of not knowing: no recorded position, a
 * position whose source never knew the total, and a stored total of zero (which cannot be divided
 * by and is not a real page count).
 */
export function progressFraction(entry: LibraryEntry): number | null {
  const progress = entry.progress;
  if (progress === null || progress.total === null || progress.total === 0) {
    return null;
  }
  // Clamped, to agree with `StoredProgress::fraction()` in the domain. A viewer can report a stale
  // total, and the core already decided what that means: a position beyond the total is *finished*
  // (1.0), not a fraction above one. Two implementations of one rule that disagree would rank the
  // same document differently depending on which side asked.
  return Math.min(1, (progress.page + 1) / progress.total);
}

/**
 * The listing in the requested order, as a new array.
 *
 * The three orders and their tie-breaks:
 *
 * - **title** — ascending, compared with the reader's locale so accented titles sort where they
 *   belong rather than by code point.
 * - **pages** — fewest first, with an unknown page count **last**. An absent count is not zero
 *   pages, and sorting it as if it were would put documents with no known length at the top.
 * - **progress** — furthest along first, with a document whose position is unknown **last**. This
 *   is deliberately not "unread at the top": `null` is not `0%`, and a document opened but never
 *   advanced holds a real position of its own.
 *
 * Every order falls back to the title and then to the row id, so two documents that share a
 * primary key come out in the same order on every render instead of depending on the sort
 * algorithm's mood. The input array is never reordered in place.
 */
export function sortEntries(
  entries: readonly LibraryEntry[],
  key: SortKey,
  locale = "en",
): LibraryEntry[] {
  const byTitle = (a: LibraryEntry, b: LibraryEntry): number =>
    displayTitle(a).localeCompare(displayTitle(b), locale);

  const byKey = (a: LibraryEntry, b: LibraryEntry): number => {
    switch (key) {
      case "title":
        return byTitle(a, b);
      case "pages":
        return compareOptionalNumber(a.pages, b.pages);
      case "progress": {
        const aFraction = progressFraction(a);
        const bFraction = progressFraction(b);
        if (aFraction === null && bFraction === null) {
          return 0;
        }
        if (aFraction === null) {
          return 1;
        }
        if (bFraction === null) {
          return -1;
        }
        // Furthest along first.
        return bFraction - aFraction;
      }
    }
  };

  return [...entries].sort((a, b) => byKey(a, b) || byTitle(a, b) || a.id - b.id);
}

/** Ascending numeric comparison with `null` sorting last, because unknown is not zero. */
function compareOptionalNumber(a: number | null, b: number | null): number {
  if (a === null && b === null) {
    return 0;
  }
  if (a === null) {
    return 1;
  }
  if (b === null) {
    return -1;
  }
  return a - b;
}

// --- Pagination -----------------------------------------------------------------------------
//
// R10 (L22): the library is paginated at ten documents per page, the user's number. The arithmetic
// lives here — a pure function over a listing already filtered and sorted — so the screen carries
// only the current page as state and no slice arithmetic hides in markup.

/** Documents per page. Ten is the user's number (R10). */
export const PAGE_SIZE = 10;

/** One page of a listing, with everything the controls need to state where the reader is. */
export interface LibraryPage<T> {
  /** The documents on this page, in the order they arrived. */
  readonly items: T[];
  /** The page actually shown, 1-based and clamped into range. */
  readonly page: number;
  /** How many pages the listing fills; never less than one, so the controls cannot say "1 of 0". */
  readonly pageCount: number;
  /** How many documents the listing holds in total. */
  readonly total: number;
}

/**
 * Cuts a whole listing into pages of `pageSize` (R10).
 *
 * It is called **after** filtering and sorting, so both apply to the entire listing before it is cut:
 * a page is a window onto the reader's filtered, sorted library, not a slice of the raw rows. Changing
 * the page therefore cannot drop a filter — the page number only chooses which window of the same
 * listing is shown, and the caller keeps the filter and the sort untouched.
 *
 * The requested `page` is clamped into `1..pageCount`, because filtering can shrink the listing while
 * the reader is several pages in; without the clamp that would show an empty page and claim the
 * reader is somewhere the listing no longer reaches. An empty listing still reports one page.
 */
export function paginate<T>(
  items: readonly T[],
  page: number,
  pageSize: number = PAGE_SIZE,
): LibraryPage<T> {
  const total = items.length;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const current = Math.min(Math.max(1, page), pageCount);
  const start = (current - 1) * pageSize;
  return { items: items.slice(start, start + pageSize), page: current, pageCount, total };
}

// --- Home categories ------------------------------------------------------------------------
//
// R4 / B3 (L20, L22): Inicio's three carousels are three readings of the stored position, and each
// reading is a rule rather than a SQL filter. The rules live here because they reuse the rules the
// card already obeys — "at the end" is the same `progressFraction(...) === 1` the card and the
// progress sort use — so a category can never disagree with the number printed on the card.
//
// **The stale category is measured from `progress.updated_at`**, which the interface receives as
// `Progress.readAt`. `documents.last_opened_at` exists in the schema but nothing ever writes it,
// so a query built on it would return confident nonsense (B3).

/** The three summary categories on Inicio. */
export type HomeCategory = "inProgress" | "finished" | "stale";

/** Every category, in the order Inicio shows them. */
export const homeCategories: readonly HomeCategory[] = ["inProgress", "finished", "stale"];

/** Whether a string names a home category. */
export function isHomeCategory(value: string): value is HomeCategory {
  return (homeCategories as readonly string[]).includes(value);
}

/**
 * How old a reading record must be to count as "not opened for a long time", in days.
 *
 * The specification names the category but not the number, so the number is a stated decision
 * rather than a hidden one: it is a single constant, it is reported with this unit, and B4 requires
 * it to be printed on the category's own screen so the reader can see the rule being applied.
 */
export const STALE_AFTER_DAYS = 30;

/** The same threshold in Unix seconds, the unit `Progress.readAt` is stored in. */
export const STALE_AFTER_SECONDS = STALE_AFTER_DAYS * 24 * 60 * 60;

/**
 * How often an open screen re-reads the clock, in milliseconds.
 *
 * A category is a function of the clock, and Inicio or a category's list can stay open for a long
 * time, so the instant cannot be captured once at mount: a document whose record crosses the
 * `STALE_AFTER_DAYS` threshold has to move carousel on its own. One **minute** is the design point
 * because the threshold is measured in days — the cost is imperceptible (one recomputation of a
 * short list per minute) and the lag is bounded to a minute, which no reader can notice for a rule
 * that changes on the order of days. A sub-second tick would recompute for no visible gain; a
 * multi-minute one would let the screen look wrong for long enough to be seen.
 */
export const CATEGORY_REFRESH_MS = 60_000;

/**
 * The current time as Unix seconds — the unit `Progress.readAt` and the `now` the category rule
 * takes both use.
 *
 * A helper rather than `Math.floor(Date.now() / 1000)` at each call site: if one caller passed
 * milliseconds, every record would silently look ancient against a seconds threshold.
 */
export function currentUnixSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

/**
 * Starts the tick that lets an open screen re-evaluate its categories, and returns the function
 * that stops it.
 *
 * Inicio and a category's list need the same interval and the same cleanup, so it lives here once
 * instead of as two copies of `setInterval` that can drift apart. The caller passes the value to
 * re-read and owns the reactive state; keeping the clock itself out of this function is what makes
 * the tick testable with fake timers, with no DOM and no dependency.
 */
export function startCategoryRefresh(onTick: () => void): () => void {
  const timer = setInterval(onTick, CATEGORY_REFRESH_MS);
  return () => clearInterval(timer);
}

/**
 * How often the running application re-reads the library listing, in milliseconds.
 *
 * The listing changes on its own while the application runs: the tracker writes a reading position every
 * few seconds from the desktop process (and writes nothing when the position has not changed), so a
 * document gains its first record, moves through its pages, and reaches its last one without anybody
 * touching the interface. Re-reading only at mount made the screen look dead for as long as the
 * application was working — the reader had to relaunch it to see Inicio change.
 *
 * **Fifteen seconds** is the design point: four small reads a minute against a local SQLite database, and a
 * lag shorter than the time it takes to read a page. Matching the tracker's own three-second tick would buy
 * freshness nothing can see, and a whole minute would leave a document that just finished sitting in "in
 * progress" long enough to be noticed.
 */
export const LIBRARY_REFRESH_MS = 15_000;

/**
 * Starts the clock that re-reads the library listing, and returns the function that stops it.
 *
 * The same shape as [`startCategoryRefresh`], and for the same reason: the caller passes what to re-read and
 * owns the state, so the clock itself is testable with fake timers and no DOM. It keeps its own constant
 * because the two answer different questions — one re-evaluates a rule measured in days, the other re-reads
 * facts that change in seconds — and one number serving both would be wrong for one of them.
 */
export function startLibraryRefresh(reload: () => void): () => void {
  const timer = setInterval(reload, LIBRARY_REFRESH_MS);
  return () => clearInterval(timer);
}

/**
 * The event that asks whoever owns the listing to re-read it (R31).
 *
 * A card action that changes a document — naming it, or restoring its own title — happens inside a
 * card, on whatever screen happens to be showing it, including the category screen whose card grid
 * this unit may not change. Threading a callback through every screen would make that screen the odd
 * one out; announcing the change lets the owner of the listing (`App`) listen once and re-read. It is
 * a plain DOM event rather than a Tauri event, because it is the interface's own change and must
 * behave the same in the plain-browser development build, where there is no application to emit one.
 */
export const LIBRARY_CHANGED_EVENT = "oct:library-changed";

/** Announces that the library changed, so its owner can re-read it. */
export function announceLibraryChanged(): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(LIBRARY_CHANGED_EVENT));
  }
}

/**
 * The documents in one home category (R4 / B3).
 *
 * `now` is passed in rather than read from the clock so the rule is testable without a browser and
 * so every carousel on one render is measured against the same instant.
 *
 * - **inProgress** — a position is recorded, its total is known, and it is not at the end.
 * - **finished** — a position is recorded and it is at the end. "At the end" is the same
 *   `progressFraction(...) === 1` the card and the progress sort already use, so the three cannot
 *   disagree about which documents are done.
 * - **stale** — a position is recorded and the READING time a source observed (`progress.read_at`) is at
 *   least `staleAfterSeconds` old. It is measured from when the person read, never from when the row was
 *   written: the sweep writes a whole library within a second of starting, so the row's own time says
 *   nothing about the reading. A record whose reading time is UNKNOWN is not stale: the application cannot
 *   claim a document was forgotten when no source ever said when it was read, and "no source said" is not
 *   "long ago".
 *
 * A document with **no** record has nothing to read in any category: it is neither in progress nor
 * finished (there is no position), and not stale either (there is no timestamp). That is the honest
 * answer rather than treating "never opened" as "opened long ago".
 *
 * **The order is part of the rule (R29): most recently read first.** The reader asked for it in their own
 * words, and it lives here rather than in the screen so the carousel, a category's own list and the
 * ten-tile cap all read one order and cannot disagree about it. What the store knows as "read" is the
 * moment the position last CHANGED (`progress.updated_at`): nothing writes `documents.last_opened_at`, so a
 * document that is opened and left on the same page does not move to the front. That is the honest reading
 * of the fact that exists, and it is the one that matters — a page turn is what makes a document the one
 * somebody is reading.
 *
 * Equal timestamps are the COMMON case rather than a corner: the launch sweep records every known document
 * within a second or two of starting, so the comparison falls back to the displayed name and then to the
 * row id. The order is therefore a function of the data alone, which is what stops the tiles from jumping:
 * this listing is re-read every fifteen seconds, and an order that could change on its own would move the
 * ten tiles under the reader's hand while they are looking at them.
 */
export function categoryEntries(
  entries: readonly LibraryEntry[],
  category: HomeCategory,
  now: number,
  staleAfterSeconds: number = STALE_AFTER_SECONDS,
): LibraryEntry[] {
  return entries
    .filter((entry) => {
      switch (category) {
        case "inProgress": {
          const fraction = progressFraction(entry);
          return fraction !== null && fraction < 1;
        }
        case "finished":
          return progressFraction(entry) === 1;
        case "stale": {
          const readAt = entry.progress?.readAt ?? null;
          return readAt !== null && now - readAt >= staleAfterSeconds;
        }
      }
    })
    .sort(byMostRecentlyRead);
}

/**
 * Most recently read first, then by the name the reader is shown, then by row id (R29).
 *
 * Every entry in a category carries a record, so a missing timestamp can only mean a record whose own
 * timestamp was never stored. It sorts to the FAR END, and the key that says so is negative infinity
 * rather than `0`: `0` is not the end of the scale, it is 1970, and a real time below it — a hand-edited
 * history, a date before the epoch — would be sorted *behind* a document whose recency nobody knows. The
 * absence of an answer is not an answer, and it must not be able to claim to be the oldest reading either.
 *
 * The name comparison is deliberately not `localeCompare`: a tiebreak must not depend on the machine's
 * locale, or two runs of the same data could order the same carousel differently. It is lowercased before
 * comparing for the same class of reason — raw code units put `Zebra` before `apple`, and a reader would
 * read that as a bug in the carousel rather than as a tiebreak — with the original spelling and then the id
 * settling the cases that differ only by case.
 */
function byMostRecentlyRead(left: LibraryEntry, right: LibraryEntry): number {
  // `-Infinity`, not `0`: see the note above. Two infinities compare equal in the guard below, so the
  // subtraction that follows can never see `Infinity - Infinity`, which would be `NaN`.
  const leftAt = left.progress?.readAt ?? Number.NEGATIVE_INFINITY;
  const rightAt = right.progress?.readAt ?? Number.NEGATIVE_INFINITY;
  if (leftAt !== rightAt) {
    return rightAt - leftAt;
  }
  const leftTitle = displayTitle(left);
  const rightTitle = displayTitle(right);
  const leftKey = leftTitle.toLowerCase();
  const rightKey = rightTitle.toLowerCase();
  if (leftKey !== rightKey) {
    return leftKey < rightKey ? -1 : 1;
  }
  if (leftTitle !== rightTitle) {
    return leftTitle < rightTitle ? -1 : 1;
  }
  return left.id - right.id;
}

/** A human name for a source or provider id, falling back to the id when none is catalogued. */
function sourceLabel(source: string, t: Translate): string {
  const key = `source.${source}`;
  const label = t(key);
  return label === key ? source : label;
}

/** A human description of a capability id, falling back to the id itself. */
function capabilityLabel(capability: string, t: Translate): string {
  const key = `capability.${capability}`;
  const label = t(key);
  return label === key ? capability : label;
}

/** Why a provider is missing, from the catalogue, or a generic reason. */
function providerReason(id: string, t: Translate): string {
  const key = `provider.reason.${id}`;
  const reason = t(key);
  return reason === key ? t("provider.reason.unknown") : reason;
}
