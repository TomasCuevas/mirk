import { describe, expect, it } from "vitest";
import { catalogues } from "./i18n";
import {
  DEFAULT_WINDOW_LAYOUT,
  gridColumns,
  isWindowLayout,
  layoutLabelKey,
  windowControls,
  windowLayouts,
} from "./window";

describe("the window controls", () => {
  it("offers exactly minimise, hide and close", () => {
    expect(windowControls.map((control) => control.action)).toEqual([
      "minimize",
      "hide",
      "close",
    ]);
  });

  it("offers no maximise control by any name", () => {
    // A2: the button does not exist, and neither does a control that would behave like one.
    const forbidden = /maximi|expand|fullscreen|full-screen/i;
    for (const control of windowControls) {
      expect(control.action).not.toMatch(forbidden);
      expect(control.command).not.toMatch(forbidden);
      expect(control.labelKey).not.toMatch(forbidden);
      expect(control.icon).not.toMatch(forbidden);
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

  // The current window is the default: an interface that was never told a layout must draw the grid the
  // reader already has, not a wider one chosen on their behalf.
  it("keeps two per row as the layout that was already in force", () => {
    expect(DEFAULT_WINDOW_LAYOUT).toBe("columns-2");
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
});
