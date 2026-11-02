/**
 * The state of the update check (S7 of `odd/tasks/ci-and-release.md`).
 *
 * The reader asked for the check to be **discreet**: the app asks GitHub for the release manifest when
 * it opens, and nothing happens on screen unless there is something to say. That makes the state machine
 * the whole of the feature's visible behaviour, so it lives here as data and pure functions instead of
 * inside the component that draws it — a component can only be read through its rendering, and these are
 * rules a rendering has to obey anyway.
 *
 * The one thing this module does NOT model is the network. Checking, downloading and relaunching are the
 * port's job (`updater.ts`); everything between an answer and a label is here.
 */
import type { Translate } from "./i18n";

/**
 * A newer version the manifest offers.
 *
 * `notes` is the release body as GitHub returns it, and it is nullable because a release may legitimately
 * have no description. The screen shows the version always and the notes when there are any — it never
 * invents a summary.
 */
export interface UpdateOffer {
  readonly version: string;
  readonly notes: string | null;
}

/**
 * What the check has to say, in the six shapes the screen draws.
 *
 * `installing` is a state and not a flag on `available` because the two answer different questions: one
 * says a newer version exists, the other says it is being fetched. A reader who sees "0.2.0 is available"
 * after pressing install would think the press did nothing.
 */
export type UpdateState =
  | { readonly kind: "idle" }
  | { readonly kind: "checking" }
  | { readonly kind: "current" }
  | { readonly kind: "available"; readonly offer: UpdateOffer }
  | { readonly kind: "installing"; readonly offer: UpdateOffer; readonly percent: number | null }
  | { readonly kind: "failed"; readonly reason: string };

/** The state after an answer: an offer, or the version already running. */
export function checked(found: UpdateOffer | null): UpdateState {
  return found === null ? { kind: "current" } : { kind: "available", offer: found };
}

/**
 * The state after a check that could not answer.
 *
 * The reason is kept, not swallowed: "no update" and "could not ask" are different facts, and reporting
 * the second as the first is how a copy that never updates stays quiet about it. Anything that is not an
 * `Error` or a string is reported as `unknown` rather than stringified — `"[object Object]"` names nothing.
 */
export function failed(error: unknown): UpdateState {
  if (error instanceof Error && error.message !== "") return { kind: "failed", reason: error.message };
  if (typeof error === "string" && error !== "") return { kind: "failed", reason: error };
  return { kind: "failed", reason: "unknown" };
}

/** The state while the offered version is being fetched and installed. */
export function installing(offer: UpdateOffer, percent: number | null): UpdateState {
  return { kind: "installing", offer, percent };
}

/**
 * Whether a newer version exists and nobody has claimed it yet.
 *
 * This is what lights the discreet mark in Ajustes. An install in progress is NOT waiting — the reader
 * has already acted — and neither is a failed check, which offered no version at all.
 */
export function isUpdateWaiting(state: UpdateState): boolean {
  return state.kind === "available";
}

/**
 * The downloaded share of the announced total, as a whole percent.
 *
 * Downloads report chunks and a manifest need not announce a total, so both "55%" and "unknown" are
 * legitimate answers here. `null` means the total is unknown — a 0% would read as "nothing has arrived"
 * forever. The result is clamped, because a server that lies about its length must not produce 104% or a
 * negative share, and rounded DOWN, so the number never claims bytes that have not arrived.
 */
export function percentOf(downloaded: number, total: number | null | undefined): number | null {
  if (typeof total !== "number" || !Number.isFinite(total) || total <= 0) return null;
  if (!Number.isFinite(downloaded)) return null;
  const share = Math.floor((downloaded / total) * 100);
  return Math.min(100, Math.max(0, share));
}

/** The one line the screen shows for a state. */
export function updateLine(state: UpdateState, t: Translate): string {
  switch (state.kind) {
    case "idle":
      return t("settings.update.idle");
    case "checking":
      return t("settings.update.checking");
    case "current":
      return t("settings.update.current");
    case "available":
      return t("settings.update.available", { version: state.offer.version });
    case "installing":
      return state.percent === null
        ? t("settings.update.installingUnknown")
        : t("settings.update.installing", { percent: state.percent });
    case "failed":
      return t("settings.update.failed", { reason: state.reason });
  }
}

/** The one control under the line, or `null` while a check or an install is already running. */
export function updateAction(
  state: UpdateState,
  t: Translate,
): { readonly kind: "check" | "install"; readonly label: string } | null {
  switch (state.kind) {
    case "checking":
    case "installing":
      return null;
    case "available":
      return { kind: "install", label: t("settings.update.install") };
    case "idle":
    case "current":
    case "failed":
      return { kind: "check", label: t("settings.update.check") };
  }
}
