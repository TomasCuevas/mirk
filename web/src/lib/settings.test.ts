import { describe, expect, it } from "vitest";
import {
  loadLanguage,
  loadWindowLayout,
  saveLanguage,
  saveWindowLayout,
  type Invoke,
} from "./settings";

/** One recorded call to the desktop process. */
interface Recorded {
  command: string;
  args?: Record<string, unknown>;
}

/**
 * A stand-in for the Rust side: it answers `get_language` with `stored` and records every call, so
 * the rules below can be checked without a Tauri application.
 */
function fakeInvoke(stored: unknown): { invoke: Invoke; calls: Recorded[] } {
  const calls: Recorded[] = [];
  const invoke = (async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    if (command === "get_language") {
      return stored as T;
    }
    return undefined as T;
  }) as Invoke;
  return { invoke, calls };
}

describe("the stored language", () => {
  // R1 (L22): the tray's words are built by the desktop process from "the stored language setting —
  // the same single source the interface reads". So the interface has to read that source when it
  // starts and write it when the user chooses a language; without this the choice is lost on every
  // launch and the tray has nothing to follow.
  it("reads the stored language at startup", async () => {
    const { invoke, calls } = fakeInvoke("es");

    expect(await loadLanguage(invoke)).toBe("es");
    expect(calls.map((call) => call.command)).toEqual(["get_language"]);
  });

  it("reports nothing stored when nothing is stored", async () => {
    const { invoke } = fakeInvoke(null);

    expect(await loadLanguage(invoke)).toBeNull();
  });

  // The store is a file another process can write. A value that is not a language this build knows
  // must not reach the screen as one: the interface would render keys it has no words for, and the
  // tray would have no labels. "Unknown" is not the same fact as "stored".
  it("does not take an unknown value for a language", async () => {
    const { invoke } = fakeInvoke("de");

    expect(await loadLanguage(invoke)).toBeNull();
  });

  it("writes the chosen language, which is what makes the tray follow it", async () => {
    const { invoke, calls } = fakeInvoke(null);

    await saveLanguage(invoke, "es");

    expect(calls).toEqual([{ command: "set_language", args: { locale: "es" } }]);
  });

  // The project's rule for errors with no screen to show them (the window actions do the same): a
  // failure is reported, never silently turned into a different fact. A failed read is not "nothing
  // is stored", and a caller that cannot tell them apart would hide a broken database.
  it("lets a failed read fail instead of passing it off as an empty store", async () => {
    const invoke = (async () => {
      throw new Error("the database could not be opened");
    }) as Invoke;

    await expect(loadLanguage(invoke)).rejects.toThrow("the database could not be opened");
  });
});

/**
 * The stored window layout (S3). Same shape as the language: one stored value, read at startup so the
 * window and the grid agree, written when the reader chooses. The desktop process applies the size; the
 * interface only has to pass the choice through and never invent one.
 */
describe("the stored window layout", () => {
  function layoutInvoke(stored: unknown): { invoke: Invoke; calls: Recorded[] } {
    const calls: Recorded[] = [];
    const invoke = (async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ command, args });
      return (command === "get_window_layout" ? stored : undefined) as T;
    }) as Invoke;
    return { invoke, calls };
  }

  it("reads the stored layout at startup", async () => {
    const { invoke, calls } = layoutInvoke("columns-3");

    expect(await loadWindowLayout(invoke)).toBe("columns-3");
    expect(calls.map((call) => call.command)).toEqual(["get_window_layout"]);
  });

  it("reports nothing stored when nothing is stored", async () => {
    const { invoke } = layoutInvoke(null);

    expect(await loadWindowLayout(invoke)).toBeNull();
  });

  // The store is a file another process can write. A value that is not one of the four layouts must not
  // reach the grid as one: "unknown" is not the same fact as "two per row", and the caller keeps the
  // default only because the answer is null.
  it("does not take an unknown value for a layout", async () => {
    const { invoke } = layoutInvoke("columns-9");

    expect(await loadWindowLayout(invoke)).toBeNull();
    expect(await loadWindowLayout(layoutInvoke(4).invoke)).toBeNull();
  });

  it("writes the chosen layout, which is what sizes the window", async () => {
    const { invoke, calls } = layoutInvoke(null);

    await saveWindowLayout(invoke, "auto");

    expect(calls).toEqual([{ command: "set_window_layout", args: { layout: "auto" } }]);
  });

  it("lets a failed read fail instead of passing it off as the default", async () => {
    const invoke = (async () => {
      throw new Error("the database could not be opened");
    }) as Invoke;

    await expect(loadWindowLayout(invoke)).rejects.toThrow("the database could not be opened");
  });
});
