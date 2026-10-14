import { describe, expect, it } from "vitest";
import { catalogues, translate } from "./i18n";
import { STALE_AFTER_DAYS, homeCategories } from "./library";
import {
  categoryDefinition,
  describeWatchedFolders,
  hashForScreen,
  hashForView,
  isScreen,
  screenFromHash,
  screenLabel,
  screens,
  viewFromHash,
  type WatchedFolder,
} from "./screens";

const english = translate("en");
const spanish = translate("es");

describe("which screen is shown", () => {
  it("resolves each screen's own hash", () => {
    expect(screenFromHash("#/home")).toBe("home");
    expect(screenFromHash("#/library")).toBe("library");
    expect(screenFromHash("#/settings")).toBe("settings");
  });

  it("falls back to Inicio for an empty or unknown hash", () => {
    // A reload or a hand-typed URL must land somewhere honest rather than on a blank page.
    expect(screenFromHash("")).toBe("home");
    expect(screenFromHash("#/")).toBe("home");
    expect(screenFromHash("#/nonsense")).toBe("home");
    expect(screenFromHash("#library")).toBe("home");
  });

  it("round-trips every screen through its hash", () => {
    for (const screen of screens) {
      expect(screenFromHash(hashForScreen(screen))).toBe(screen);
    }
  });

  it("recognises only the screens it has", () => {
    for (const screen of screens) {
      expect(isScreen(screen)).toBe(true);
    }
    expect(isScreen("home2")).toBe(false);
    expect(isScreen("")).toBe(false);
  });

  it("labels every screen in both languages", () => {
    for (const screen of screens) {
      const label = screenLabel(screen, english);
      expect(label).not.toBe(screen);
      expect(screenLabel(screen, spanish)).not.toBe(label);
    }
  });

  it("resolves the navbar screens to screen views", () => {
    for (const screen of screens) {
      expect(viewFromHash(hashForScreen(screen))).toEqual({ kind: "screen", screen });
    }
  });
});

describe("the home categories", () => {
  it("gives every category a screen of its own that its hash round-trips", () => {
    // B2: the carousel's button opens a full list on a screen of its own, so a category is a
    // destination the hash can name and come back from.
    for (const category of homeCategories) {
      const view = { kind: "category", category } as const;
      expect(viewFromHash(hashForView(view))).toEqual(view);
    }
  });

  it("falls back to Inicio for an unknown category, like an unknown screen", () => {
    expect(viewFromHash("#/category/nonsense")).toEqual({ kind: "screen", screen: "home" });
  });

  it("states every category's definition, and the stale threshold with it", () => {
    // B4: each category says what it means on screen, and "a long time" is a number the reader can
    // read rather than one hidden in the code (B3).
    for (const category of homeCategories) {
      expect(categoryDefinition(category, english).trim()).not.toBe("");
      expect(categoryDefinition(category, spanish)).not.toBe(
        categoryDefinition(category, english),
      );
    }
    expect(categoryDefinition("stale", english)).toContain(String(STALE_AFTER_DAYS));
    expect(categoryDefinition("stale", spanish)).toContain(String(STALE_AFTER_DAYS));
  });
});

describe("watched folders", () => {
  function folder(overrides: Partial<WatchedFolder> = {}): WatchedFolder {
    return { id: 1, path: "/library", recursive: true, enabled: true, ...overrides };
  }

  it("distinguishes a pending load from a genuinely empty list", () => {
    expect(describeWatchedFolders(null, english)).toBe(
      "Reading the folders the store has recorded…",
    );
    expect(describeWatchedFolders([], english)).toBe(
      "No folder has been recorded yet. A scan records one.",
    );
  });

  it("has no message once a folder exists", () => {
    expect(describeWatchedFolders([folder()], english)).toBeNull();
  });
});

describe("the string catalogue", () => {
  it("holds the same keys in both languages, and no blank value", () => {
    // S6: every new string goes through the catalogue in both languages. A key present in one
    // catalogue and missing from the other would silently fall back to English.
    expect(Object.keys(catalogues.es).sort()).toEqual(Object.keys(catalogues.en).sort());
    for (const [key, value] of Object.entries(catalogues.en)) {
      expect(value.trim(), `English value for ${key}`).not.toBe("");
      expect(catalogues.es[key]?.trim(), `Spanish value for ${key}`).not.toBe("");
    }
  });
});
