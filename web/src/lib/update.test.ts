import { describe, expect, it } from "vitest";
import { translate } from "./i18n";
import {
  checked,
  failed,
  installing,
  isUpdateWaiting,
  percentOf,
  updateAction,
  updateLine,
  type UpdateOffer,
  type UpdateState,
} from "./update";

const english = translate("en");
const spanish = translate("es");

const offer: UpdateOffer = { version: "0.2.0", notes: "Faster covers." };

/** Every state the screen can be in, so a message or an action cannot be forgotten for one of them. */
const states: readonly UpdateState[] = [
  { kind: "idle" },
  { kind: "checking" },
  { kind: "current" },
  { kind: "available", offer },
  { kind: "installing", offer, percent: 42 },
  { kind: "installing", offer, percent: null },
  { kind: "failed", reason: "the endpoint answered 404" },
];

/**
 * S7 of `odd/tasks/ci-and-release.md`. The check is silent until it finds something, so the state
 * machine is the whole of the feature's visible behaviour, and it is pinned here rather than inside a
 * component: a component can only be read through its rendering, and these are the rules a rendering
 * has to obey anyway.
 */
describe("the update check's states", () => {
  // The one distinction the discreet notice turns on: a newer version exists and nobody has acted on
  // it yet. Once the reader is installing it, a dot that still says "there is something to do" would be
  // pointing at work already in progress.
  it("counts as waiting only while a newer version sits unclaimed", () => {
    expect(isUpdateWaiting({ kind: "available", offer })).toBe(true);
    expect(isUpdateWaiting({ kind: "installing", offer, percent: 10 })).toBe(false);
    expect(isUpdateWaiting({ kind: "current" })).toBe(false);
    expect(isUpdateWaiting({ kind: "idle" })).toBe(false);
    expect(isUpdateWaiting({ kind: "checking" })).toBe(false);
    // A failed check is NOT waiting: nothing was offered, and claiming otherwise would ask the reader
    // to act on a version that was never named.
    expect(isUpdateWaiting({ kind: "failed", reason: "offline" })).toBe(false);
  });

  it("turns the check's answer into an offer or a current version", () => {
    expect(checked(null)).toEqual({ kind: "current" });
    expect(checked(offer)).toEqual({ kind: "available", offer });
  });

  // The check can fail for reasons the interface cannot fix (no network, a missing manifest, a
  // signature that does not verify). It is reported with the reason it gave and never as "current".
  it("keeps the reason a check failed, whatever it was thrown as", () => {
    expect(failed(new Error("the endpoint answered 404"))).toEqual({
      kind: "failed",
      reason: "the endpoint answered 404",
    });
    expect(failed("offline")).toEqual({ kind: "failed", reason: "offline" });
    // Anything else is named as unknown rather than stringified into "[object Object]".
    expect(failed({ odd: true })).toEqual({ kind: "failed", reason: "unknown" });
    expect(failed(undefined)).toEqual({ kind: "failed", reason: "unknown" });
  });

  it("gives the install its own state, carrying the percentage when it is known", () => {
    expect(installing(offer, 42)).toEqual({ kind: "installing", offer, percent: 42 });
    expect(installing(offer, null)).toEqual({ kind: "installing", offer, percent: null });
  });
});

/**
 * A download reports chunks, not percentages, and a manifest need not announce a total. Both cases are
 * turned into one number here so the screen never divides anything itself.
 */
describe("the download's percentage", () => {
  it("is a whole share of the announced total", () => {
    expect(percentOf(0, 1000)).toBe(0);
    expect(percentOf(500, 1000)).toBe(50);
    expect(percentOf(1000, 1000)).toBe(100);
    // Rounding down never claims a share that has not arrived; 999/1000 is 99%, not "done".
    expect(percentOf(999, 1000)).toBe(99);
    expect(percentOf(1, 3)).toBe(33);
  });

  // Unknown total: the plugin omits `contentLength` when the server sends no length. A percentage
  // cannot be invented, and 0% would read as "nothing has arrived" forever.
  it("is unknown when the total is unknown or zero", () => {
    expect(percentOf(500, null)).toBeNull();
    expect(percentOf(500, undefined)).toBeNull();
    expect(percentOf(500, 0)).toBeNull();
  });

  // A server that lies about the total, or a chunk that overshoots it, must not produce 104% or -3%.
  it("stays inside 0 and 100", () => {
    expect(percentOf(1200, 1000)).toBe(100);
    expect(percentOf(-10, 1000)).toBe(0);
  });
});

/**
 * The words. They are asserted for both languages, because a translated key that is missing renders as
 * the key itself and a reader would see `settings.update.available` on the screen.
 */
describe("the update check's words", () => {
  it("says one line per state, with the version and the reason inside it", () => {
    expect(updateLine({ kind: "idle" }, english)).toBe(english("settings.update.idle"));
    expect(updateLine({ kind: "checking" }, english)).toBe(english("settings.update.checking"));
    expect(updateLine({ kind: "current" }, english)).toBe(english("settings.update.current"));
    expect(updateLine({ kind: "available", offer }, english)).toBe(
      english("settings.update.available", { version: "0.2.0" }),
    );
    expect(updateLine({ kind: "installing", offer, percent: 42 }, english)).toBe(
      english("settings.update.installing", { percent: 42 }),
    );
    expect(updateLine({ kind: "installing", offer, percent: null }, english)).toBe(
      english("settings.update.installingUnknown"),
    );
    expect(updateLine({ kind: "failed", reason: "offline" }, english)).toBe(
      english("settings.update.failed", { reason: "offline" }),
    );
  });

  it("names the version and the reason in Spanish too, instead of leaking the key", () => {
    const available = updateLine({ kind: "available", offer }, spanish);
    const failure = updateLine({ kind: "failed", reason: "sin conexión" }, spanish);

    expect(available).toContain("0.2.0");
    expect(failure).toContain("sin conexión");
    for (const line of [available, failure]) {
      expect(line.startsWith("settings.")).toBe(false);
    }
  });

  // Every state must have words. A missing key comes back as the key itself, and this is the check
  // that fails when a state is added to the union and its message is forgotten.
  it("has words for every state in both languages", () => {
    for (const state of states) {
      for (const t of [english, spanish]) {
        expect(updateLine(state, t).startsWith("settings.")).toBe(false);
      }
    }
  });
});

/**
 * The one control under the line. It is a small description rather than a label, because what it does
 * depends on the state: ask, install, or nothing while work is already running.
 */
describe("the update check's control", () => {
  it("starts no action while a check or an install is running", () => {
    expect(updateAction({ kind: "checking" }, english)).toBeNull();
    expect(updateAction({ kind: "installing", offer, percent: 5 }, english)).toBeNull();
  });

  it("asks for a check from rest, and from a previous answer", () => {
    for (const state of [
      { kind: "idle" },
      { kind: "current" },
      { kind: "failed", reason: "offline" },
    ] as const) {
      expect(updateAction(state, english)).toEqual({
        kind: "check",
        label: english("settings.update.check"),
      });
    }
  });

  it("offers to install the version it named", () => {
    expect(updateAction({ kind: "available", offer }, english)).toEqual({
      kind: "install",
      label: english("settings.update.install"),
    });
  });
});
