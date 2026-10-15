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
  /** Unix seconds when the position was recorded, or null. */
  recordedAt: number | null;
}

/** One document in the library view. */
export interface LibraryEntry {
  id: number;
  path: string;
  title: string | null;
  author: string | null;
  /** Page count from the scanner; null when it was never established. */
  pages: number | null;
  progress: Progress | null;
}

/** What the `open_containing_folder` command returns when the folder is opened. */
export interface OpenedFolder {
  folder: string;
  viewer: string;
}

/** The result of a card's "open containing folder" action, so the card can report it. */
export type FolderActionStatus =
  | { id: number; kind: "ok"; folder: string; viewer: string }
  | { id: number; kind: "error"; message: string };

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

/** What the card draws and says about reading progress (R7). */
export interface ProgressView {
  /** The bar's fill, `0..1`. `0` when nothing is recorded, which is what the card was asked to show. */
  readonly fraction: number;
  /** The text over the bar: the whole percentage, or the page when no total is known. */
  readonly marker: string;
  /** Where the marker sits along the bar, `0..1` — the fraction itself, so the number states the
   *  fraction rather than merely sitting next to it. */
  readonly markerAt: number;
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
      provenance,
    };
  }

  return {
    fraction,
    marker: t("card.progressPercent", { percent: Math.round(fraction * 100) }),
    markerAt: fraction,
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
 * The name to show when a document carries no embedded title.
 *
 * Everything after the last separator, minus the extension — the file's own name, which is what the
 * reader sees in their file manager and what they will recognise. A path is not a title: rendering
 * `/home/u/libros/report.pdf` where a title belongs tells the reader nothing they cannot already see
 * and reads like a mistake.
 *
 * It is deliberately **not** marked on the card. The card shows the file's name as its title, which is
 * the ordinary convention, and the distinction that matters — "this document has no title of its own"
 * — belongs where the reader can act on it, in the title editor, not as a badge on every cover.
 */
export function titleFromPath(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const extension = name.lastIndexOf(".");
  // `> 0` rather than `>= 0`, so a leading-dot name like `.hidden` keeps its dot.
  return extension > 0 ? name.slice(0, extension) : name;
}

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
 * The title a card shows.
 *
 * The embedded title when the document has one, and the file's own name otherwise — never the
 * path. This is the rule L18 shipped as a fix for real documents whose PDFs carry no title, and it
 * is the one place the fallback lives so no screen can render a path where a title belongs.
 *
 * A blank or whitespace-only embedded title counts as no title: PDF metadata can hold an empty
 * string, and a blank card is no more helpful than a path.
 */
export function displayTitle(entry: LibraryEntry): string {
  const title = entry.title?.trim() ?? "";
  return title === "" ? titleFromPath(entry.path) : title;
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
// `Progress.recordedAt`. `documents.last_opened_at` exists in the schema but nothing ever writes it,
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

/** The same threshold in Unix seconds, the unit `Progress.recordedAt` is stored in. */
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
 * The current time as Unix seconds — the unit `Progress.recordedAt` and the `now` the category rule
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
 * The documents in one home category (R4 / B3).
 *
 * `now` is passed in rather than read from the clock so the rule is testable without a browser and
 * so every carousel on one render is measured against the same instant.
 *
 * - **inProgress** — a position is recorded, its total is known, and it is not at the end.
 * - **finished** — a position is recorded and it is at the end. "At the end" is the same
 *   `progressFraction(...) === 1` the card and the progress sort already use, so the three cannot
 *   disagree about which documents are done.
 * - **stale** — a position is recorded and its own timestamp (`progress.updated_at`) is at least
 *   `staleAfterSeconds` old. This does not depend on a known total: a position without a total is
 *   still a record with a time.
 *
 * A document with **no** record has nothing to read in any category: it is neither in progress nor
 * finished (there is no position), and not stale either (there is no timestamp). That is the honest
 * answer rather than treating "never opened" as "opened long ago".
 */
export function categoryEntries(
  entries: readonly LibraryEntry[],
  category: HomeCategory,
  now: number,
  staleAfterSeconds: number = STALE_AFTER_SECONDS,
): LibraryEntry[] {
  return entries.filter((entry) => {
    switch (category) {
      case "inProgress": {
        const fraction = progressFraction(entry);
        return fraction !== null && fraction < 1;
      }
      case "finished":
        return progressFraction(entry) === 1;
      case "stale": {
        const recordedAt = entry.progress?.recordedAt ?? null;
        return recordedAt !== null && now - recordedAt >= staleAfterSeconds;
      }
    }
  });
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
