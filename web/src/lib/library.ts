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
 * The one-line reading-progress statement for a document.
 *
 * `"p. 12 of 80, recorded by Evince"` — position first, provenance second, never a percentage.
 * A `null` progress is a document nobody has read, not a zero.
 */
export function describeEntryProgress(progress: Progress | null, t: Translate): string {
  if (progress === null) {
    return t("progress.none");
  }

  const position =
    progress.total === null
      ? t("progress.position", { page: progress.page + 1 })
      : t("progress.positionOf", { page: progress.page + 1, total: progress.total });

  return `${position}, ${describeProvenance(progress, t)}`;
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
