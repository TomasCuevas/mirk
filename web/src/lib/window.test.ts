import { describe, expect, it } from "vitest";
import { catalogues } from "./i18n";
import {
  DEFAULT_WINDOW_LAYOUT,
  gridCardMinimum,
  gridColumns,
  interfaceFontSize,
  isWindowLayout,
  layoutLabelKey,
  libraryPageSize,
  loadWindowPreset,
  navMode,
  RESTORE_WINDOW_LAYOUT,
  WINDOW_LAYOUT_CHANGED_EVENT,
  windowControls,
  windowLayouts,
  type PresetInvoke,
} from "./window";

describe("the window controls", () => {
  // S4 of `odd/tasks/desktop-chrome.md` and R6 of the same record: the four controls, minimise first
  // and the tray control beside it, as the user asked.
  it("offers minimise, hide, maximise and close, in that order", () => {
    expect(windowControls.map((control) => control.action)).toEqual([
      "minimize",
      "hide",
      "maximize",
      "close",
    ]);
  });

  it("adds no control besides those four", () => {
    const forbidden = /expand|fullscreen|full-screen|pin|always-on-top/i;
    for (const control of windowControls) {
      expect(control.action).not.toMatch(forbidden);
      expect(control.command).not.toMatch(forbidden);
      expect(control.labelKey).not.toMatch(forbidden);
      expect(control.icon).not.toMatch(forbidden);
    }
  });

  // The maximise control is the only stateful one: its words change with the window, so both keys
  // must exist in both catalogues even though only one is on screen at a time.
  it("names both states of the maximise control", () => {
    for (const key of ["window.maximize", "window.restore"]) {
      expect(catalogues.en[key]?.trim(), key).toBeTruthy();
      expect(catalogues.es[key]?.trim(), key).toBeTruthy();
      expect(catalogues.es[key], key).not.toBe(catalogues.en[key]);
    }
  });

  it("calls a distinct command per control", () => {
    const commands = windowControls.map((control) => control.command);
    expect(new Set(commands).size).toBe(commands.length);
  });

  it("labels every control in both languages", () => {
    for (const control of windowControls) {
      expect(catalogues.en[control.labelKey]?.trim()).toBeTruthy();
      expect(catalogues.es[control.labelKey]?.trim()).toBeTruthy();
      expect(control.icon.trim()).not.toBe("");
    }
  });
});

/**
 * The four layouts the reader can choose (S2, S4).
 *
 * The window's pixel size is the desktop process's business, because it is the process that owns the
 * window; what the interface must get right is the column count, since a preset that widened the window
 * to fit three cards while the grid still auto-filled to two would answer half the request. `auto` is
 * the one layout with no count: full screen shows as many cards as fit at the card's declared minimum.
 */
describe("the window layouts", () => {
  it("offers exactly the four layouts, two per row first and full screen last", () => {
    expect(windowLayouts).toEqual(["columns-2", "columns-3", "columns-4", "auto"]);
  });

  // R7 of `odd/tasks/desktop-chrome.md`: full screen is what a store that has never chosen opens with.
  it("opens on full screen when nothing is stored", () => {
    expect(DEFAULT_WINDOW_LAYOUT).toBe("auto");
  });

  // R7: the default is full screen, so "restore" cannot fall back to it — the window comes back to
  // three per row, which is the layout the user named.
  it("restores to three per row, which is not the default", () => {
    expect(RESTORE_WINDOW_LAYOUT).toBe("columns-3");
    expect(RESTORE_WINDOW_LAYOUT).not.toBe(DEFAULT_WINDOW_LAYOUT);
  });

  it("turns a preset into the exact column count it names", () => {
    expect(gridColumns("columns-2")).toBe("2");
    expect(gridColumns("columns-3")).toBe("3");
    expect(gridColumns("columns-4")).toBe("4");
  });

  // R16's carousel rule and S4: full screen is the one layout that does not count — the grid fills the
  // width with as many tiles as the card's minimum permits.
  it("lets full screen fill the width instead of naming a count", () => {
    expect(gridColumns("auto")).toBe("auto-fill");
  });

  // S5: Ajustes offers the four choices by name, so each one needs words in both catalogues and no two
  // choices may share a label — "Per row" three times would not say which one is which.
  it("labels every layout in both languages, differently", () => {
    const labels = windowLayouts.map((layout) => layoutLabelKey(layout));
    expect(new Set(labels).size).toBe(windowLayouts.length);

    for (const [index, key] of labels.entries()) {
      expect(catalogues.en[key]?.trim(), key).toBeTruthy();
      expect(catalogues.es[key]?.trim(), key).toBeTruthy();
      expect(catalogues.es[key], key).not.toBe(catalogues.en[key]);
      expect(labels.indexOf(key), `"${key}" must be its own layout's label`).toBe(index);
    }
  });

  // L15 of `odd/tasks/responsive.md`: full screen is read from farther away, so the whole interface is
  // drawn on a larger root. Every rem-based token — type, spacing, control heights, the sidebar — follows
  // it, which is the point: a single scale instead of a dozen per-component media queries.
  it("grows the interface only in full screen", () => {
    expect(interfaceFontSize("auto")).toBe(18);
    expect(interfaceFontSize("columns-2")).toBe(16);
    expect(interfaceFontSize("columns-3")).toBe(16);
    expect(interfaceFontSize("columns-4")).toBe(16);
  });

  // L10 of `odd/tasks/window-layout.md`: full screen used to auto-fill at the card's bare 8.5 rem minimum,
  // which fit a dozen tiny cards on a large screen. Its floor is now the width two columns have in the
  // shipped window, so full screen shows cards the size the fixed layouts show — just as many as fit.
  it("gives full screen the fixed cards' floor instead of the bare minimum", () => {
    const fixed = gridCardMinimum("columns-2");
    expect(gridCardMinimum("columns-3")).toBe(fixed);
    expect(gridCardMinimum("columns-4")).toBe(fixed);
    expect(fixed).toBe(136);

    const full = gridCardMinimum("auto");
    expect(full).toBeGreaterThan(fixed);
    // (430 shipped window − 32 padding − 9.6 gap) / 2, scaled by the larger full-screen root (L15): the
    // cards keep their proportion to the text, not only to the window.
    expect(full).toBeCloseTo(((430 - 32 - 9.6) / 2) * (18 / 16), 5);
  });

  // L11: the page size follows the row, because a page that ends mid-row leaves holes a reader reads as
  // missing documents. Two per row keeps the ten the library always had; three and four hold twelve; full
  // screen, with the most room, holds fourteen.
  it("sizes the library page to the row it draws", () => {
    expect(libraryPageSize("columns-2")).toBe(10);
    expect(libraryPageSize("columns-3")).toBe(12);
    expect(libraryPageSize("columns-4")).toBe(12);
    expect(libraryPageSize("auto")).toBe(14);
  });

  // S3 of `odd/tasks/navigation.md`: the navigation follows the window's layout. Full screen moves it to
  // the left sidebar; every windowed layout keeps the bottom bar, however wide it is.
  it("puts the navigation in a sidebar only in full screen", () => {
    expect(navMode("auto")).toBe("side");
    expect(navMode("columns-2")).toBe("bottom");
    expect(navMode("columns-3")).toBe("bottom");
    expect(navMode("columns-4")).toBe("bottom");
  });

  it("recognises only the layouts it offers", () => {
    for (const layout of windowLayouts) {
      expect(isWindowLayout(layout)).toBe(true);
    }
    expect(isWindowLayout("columns-5")).toBe(false);
    expect(isWindowLayout("grid")).toBe(false);
    expect(isWindowLayout("")).toBe(false);
    expect(isWindowLayout(null)).toBe(false);
    expect(isWindowLayout(3)).toBe(false);
  });

  // T3: the desktop process emits this name and the interface listens for it, so the literal is pinned
  // here — the two halves live in different languages and a rename on one side would be silent.
  it("names the event the desktop process emits when it changes the layout", () => {
    expect(WINDOW_LAYOUT_CHANGED_EVENT).toBe("window-layout-changed");
  });
});

/**
 * S3: the preset the interface reads at startup, beside the stored layout.
 *
 * The desktop process resolves its own fallback, so a windowed key is the whole answer, and the same key
 * is what its drag gesture uses. `auto` or a word from another build is not a preset this build can
 * come back to: the caller keeps its own fallback rather than adopting a value the two halves disagree
 * about. A failed read is not "nothing remembered" either — it is handed on so the caller can report it.
 */
describe("the remembered window preset", () => {
  function presetInvoke(answer: unknown): { invoke: PresetInvoke; commands: string[] } {
    const commands: string[] = [];
    const invoke = (async <T,>(command: string): Promise<T> => {
      commands.push(command);
      return answer as T;
    }) as PresetInvoke;
    return { invoke, commands };
  }

  it("reads a windowed preset through", async () => {
    for (const layout of ["columns-2", "columns-3", "columns-4"] as const) {
      const { invoke } = presetInvoke(layout);
      expect(await loadWindowPreset(invoke)).toBe(layout);
    }
  });

  it("asks the desktop process for the preset, once", async () => {
    const { invoke, commands } = presetInvoke("columns-4");

    await loadWindowPreset(invoke);

    expect(commands).toEqual(["get_window_preset"]);
  });

  // Full screen is not a preset: the process never stores `auto` here, so an answer of `auto` means the
  // halves have drifted and the interface must not treat it as the rows to come back to.
  it("does not take full screen for a preset", async () => {
    const { invoke } = presetInvoke("auto");

    expect(await loadWindowPreset(invoke)).toBeNull();
  });

  it("does not take an unknown value for a preset", async () => {
    for (const answer of ["columns-9", "", 4, null, undefined]) {
      const { invoke } = presetInvoke(answer);
      expect(await loadWindowPreset(invoke), String(answer)).toBeNull();
    }
  });

  // The same rule the stored layout is read under: a broken store must not look like a reader who never
  // chose a preset, or the interface would silently fall back to three per row over a real answer.
  it("lets a failed read fail instead of passing it off as nothing remembered", async () => {
    const invoke = (async () => {
      throw new Error("the database could not be opened");
    }) as PresetInvoke;

    await expect(loadWindowPreset(invoke)).rejects.toThrow("the database could not be opened");
  });
});
