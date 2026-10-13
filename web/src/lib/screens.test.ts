import { describe, expect, it } from "vitest";
import { catalogues, translate } from "./i18n";
import {
  describeWatchedFolders,
  hashForScreen,
  homeSections,
  isScreen,
  screenFromHash,
  screenLabel,
  screens,
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
});

describe("Inicio as an honest placeholder", () => {
  it("says what it will show, naming all three summaries", () => {
    const sections = homeSections(english);
    expect(sections).toHaveLength(3);
    const text = sections.map((section) => `${section.title} ${section.detail}`).join(" ");
    expect(text).toMatch(/progress/i);
    expect(text).toMatch(/finish/i);
    expect(text).toMatch(/long time/i);
  });

  it("invents no number, because no query produces one yet", () => {
    for (const section of homeSections(english)) {
      expect(section.title).not.toMatch(/\d/);
      expect(section.detail).not.toMatch(/\d/);
    }
  });

  it("has every section in Spanish as well", () => {
    const spanishText = homeSections(spanish)
      .map((section) => `${section.title} ${section.detail}`)
      .join(" ");
    expect(spanishText).not.toBe(
      homeSections(english)
        .map((section) => `${section.title} ${section.detail}`)
        .join(" "),
    );
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
