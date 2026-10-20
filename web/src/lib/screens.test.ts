import { describe, expect, it } from "vitest";
import { catalogues, translate } from "./i18n";
import { STALE_AFTER_DAYS, homeCategories } from "./library";
import {
  categoryDefinition,
  coveringFolder,
  describeCoveredFolder,
  describeRemoval,
  describeWatchedFolders,
  hashForScreen,
  hashForView,
  isScreen,
  isSettingsSection,
  navbarScreen,
  screenFromHash,
  screenLabel,
  screens,
  settingsSectionPurpose,
  settingsSectionSummary,
  settingsSectionTitle,
  settingsSections,
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

/** How many sentences a value holds, by the rule the "one plain sentence" claim is read with. */
function sentenceCount(value: string): number {
  return value.split(/(?<=[.!?…])\s+/u).filter((part) => part.trim() !== "").length;
}

/**
 * R43 (L71). Ajustes is an index of three sub-screens, and each one is a destination of its own.
 *
 * The user's report was *"no es nada intuitiva actualmente"* and the shape they chose when asked was
 * three sub-screens — Biblioteca, Lectura, Aplicación — so a sub-screen has to be a `View` the hash can
 * name and come back from, exactly as a home category already is. The tests below pin the routes, the
 * honest fallback, and the words: a section with no name, or with three sentences doing one job, is the
 * wall the round was opened about.
 */
describe("Ajustes as an index of three sub-screens", () => {
  // S1: three rows, in the reader's order — Biblioteca, Lectura, Aplicación.
  it("names the three sections in the order the reader chose", () => {
    expect(settingsSections).toEqual(["library", "reading", "app"]);
  });

  // S2: a sub-screen is addressable by hash, so a reload keeps the reader where they were.
  it("is reachable at the three addresses the spec names", () => {
    expect(hashForView({ kind: "settings", section: "library" })).toBe("#/settings/library");
    expect(hashForView({ kind: "settings", section: "reading" })).toBe("#/settings/reading");
    expect(hashForView({ kind: "settings", section: "app" })).toBe("#/settings/app");

    expect(viewFromHash("#/settings/library")).toEqual({ kind: "settings", section: "library" });
    expect(viewFromHash("#/settings/reading")).toEqual({ kind: "settings", section: "reading" });
    expect(viewFromHash("#/settings/app")).toEqual({ kind: "settings", section: "app" });
  });

  it("round-trips every sub-screen through its hash, like every category", () => {
    for (const section of settingsSections) {
      const view = { kind: "settings", section } as const;
      expect(viewFromHash(hashForView(view))).toEqual(view);
    }
  });

  // A hash nobody wrote falls back honestly: the reader asked for Ajustes and gets Ajustes, with its
  // three choices in front of them — not a blank page and not Inicio.
  it("falls back to the index for an unknown sub-screen", () => {
    expect(viewFromHash("#/settings/nonsense")).toEqual({ kind: "screen", screen: "settings" });
    expect(viewFromHash("#/settings/")).toEqual({ kind: "screen", screen: "settings" });
    // The index itself is still a plain screen, and still round-trips.
    expect(viewFromHash("#/settings")).toEqual({ kind: "screen", screen: "settings" });
    expect(hashForView({ kind: "screen", screen: "settings" })).toBe("#/settings");
  });

  it("recognises only the sub-screens it has", () => {
    for (const section of settingsSections) {
      expect(isSettingsSection(section)).toBe(true);
    }
    expect(isSettingsSection("library2")).toBe(false);
    expect(isSettingsSection("")).toBe(false);
    expect(isSettingsSection("category")).toBe(false);
  });

  // S1 and S4: each row carries its name and ONE plain sentence saying what lives inside, in both
  // languages. A summary that is a second heading, or that runs on into a paragraph, is the wall again.
  it("names every section and says what lives inside it in one sentence", () => {
    for (const section of settingsSections) {
      const title = settingsSectionTitle(section, english);
      const summary = settingsSectionSummary(section, english);

      expect(title.trim(), section).not.toBe("");
      expect(summary.trim(), section).not.toBe("");
      expect(summary, section).not.toBe(title);
      expect(sentenceCount(summary), `${section}: "${summary}"`).toBe(1);

      expect(settingsSectionTitle(section, spanish), section).not.toBe(title);
      expect(settingsSectionSummary(section, spanish), section).not.toBe(summary);
    }
  });

  // S4: the sub-screen itself states one line saying what it is FOR, which is a different sentence from
  // the index's summary — one names the contents, the other explains the purpose.
  it("gives every sub-screen a line saying what it is for", () => {
    for (const section of settingsSections) {
      const purpose = settingsSectionPurpose(section, english);

      expect(purpose.trim(), section).not.toBe("");
      expect(sentenceCount(purpose), `${section}: "${purpose}"`).toBe(1);
      expect(purpose, section).not.toBe(settingsSectionSummary(section, english));
      expect(settingsSectionPurpose(section, spanish), section).not.toBe(purpose);
    }
  });

  // The three sentences of one section must say three different things: three copies of "Library" is
  // the index the user complained about, only smaller.
  it("says its own thing in the index's summary and in the sub-screen's purpose", () => {
    for (const section of settingsSections) {
      const words = [
        settingsSectionTitle(section, english),
        settingsSectionSummary(section, english),
        settingsSectionPurpose(section, english),
      ];
      expect(new Set(words).size, section).toBe(3);
    }
  });

  // B2, restated for the third kind of view: a sub-screen belongs to Ajustes, so the navbar keeps
  // Ajustes marked while the reader is inside one, exactly as a category keeps Inicio marked.
  it("keeps the section marked in the navbar while a sub-screen is open", () => {
    for (const section of settingsSections) {
      expect(navbarScreen({ kind: "settings", section })).toBe("settings");
    }
  });

  it("keeps Inicio marked for a category and names every screen as itself", () => {
    for (const category of homeCategories) {
      expect(navbarScreen({ kind: "category", category })).toBe("home");
    }
    for (const screen of screens) {
      expect(navbarScreen({ kind: "screen", screen })).toBe(screen);
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
    // B4, still true of the CATEGORY screen: it says what the category means, and "a long time" is a
    // number the reader can read rather than one hidden in the code (B3). Inicio's carousels stopped
    // stating it at the reader's request (L70); this screen kept it.
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

describe("a folder already covered by a watched one", () => {
  const watched = (id: number, path: string): WatchedFolder => ({
    id,
    path,
    recursive: true,
    enabled: true,
  });

  // R22 (L41): the user asked that adding a subfolder of a watched folder be refused with an alert
  // explaining why. This is the rule the interface uses to explain it in the reader's own language.
  it("names the folder that covers a subfolder of it", () => {
    const folders = [watched(1, "/lib/pdfs"), watched(2, "/other")];

    expect(coveringFolder(folders, "/lib/pdfs/novelas")?.path).toBe("/lib/pdfs");
  });

  // The trap this rule exists to avoid: a string-prefix comparison would call `/lib/pdfs-old` inside
  // `/lib/pdfs`, refuse a folder the person is entitled to watch, and say something untrue about why.
  it("treats a sibling that shares a prefix as unrelated", () => {
    expect(coveringFolder([watched(1, "/lib/pdfs")], "/lib/pdfs-old")).toBeNull();
  });

  it("covers the same folder, deeper ones, and nothing unrelated", () => {
    const folders = [watched(1, "/lib/pdfs")];

    expect(coveringFolder(folders, "/lib/pdfs")?.id).toBe(1);
    expect(coveringFolder(folders, "/lib/pdfs/")?.id).toBe(1);
    expect(coveringFolder(folders, "/lib/pdfs/2024/enero")?.id).toBe(1);
    expect(coveringFolder(folders, "/music")).toBeNull();
    expect(coveringFolder(folders, "/lib")).toBeNull();
  });
});

describe("the sentence that explains a refusal", () => {
  const folder = (id: number, path: string): WatchedFolder => ({
    id,
    path,
    recursive: true,
    enabled: true,
  });

  // #56 (R26): the store's own UNIQUE index would refuse the duplicate anyway, but the covering folder
  // would be the chosen path itself, and the sentence read "that folder is inside <the folder you just
  // chose>". True, and useless. Being already watched is its own sentence.
  it("says a folder is already watched when it covers itself", () => {
    const watched = folder(1, "/lib/pdfs");

    expect(describeCoveredFolder(watched, "/lib/pdfs", english)).toBe(
      "That folder is already watched, so it was not added.",
    );
    // A trailing separator is the same folder, not a different one.
    expect(describeCoveredFolder(watched, "/lib/pdfs/", english)).toBe(
      "That folder is already watched, so it was not added.",
    );
  });

  it("names the covering folder when it is a different one", () => {
    const watched = folder(1, "/lib/pdfs");

    expect(describeCoveredFolder(watched, "/lib/pdfs/novelas", english)).toBe(
      "That folder is inside /lib/pdfs, which is already watched, so it was not added.",
    );
  });

  it("says it in Spanish too", () => {
    const watched = folder(1, "/lib/pdfs");

    expect(describeCoveredFolder(watched, "/lib/pdfs", spanish)).toBe(
      "Esa carpeta ya está vigilada, así que no se agregó.",
    );
    expect(describeCoveredFolder(watched, "/lib/pdfs/novelas", spanish)).toContain(
      "está dentro de /lib/pdfs",
    );
  });
});

describe("what a removal takes with it", () => {
  const removal = (documents: number, kept = 0) => ({
    folder: { id: 1, path: "/lib/pdfs", recursive: true, enabled: true },
    documents: Array.from({ length: documents }, (_, index) => `/lib/pdfs/${index}.pdf`),
    kept: Array.from({ length: kept }, (_, index) => `/lib/inner/${index}.pdf`),
  });

  // R27 (L42): the confirmation states how much leaves the library before anything happens, and the
  // count comes from the store's own plan rather than from a guess the screen makes about it.
  it("states the count, and says one as one", () => {
    expect(describeRemoval(removal(1), english)).toBe(
      "1 document of this folder leaves your library.",
    );
    expect(describeRemoval(removal(7), english)).toBe(
      "7 documents of this folder leave your library.",
    );
    expect(describeRemoval(removal(1), spanish)).toBe(
      "1 documento de esta carpeta sale de su biblioteca.",
    );
  });

  // The guard creates a second way for nothing to leave, and it is NOT the same fact: the documents are
  // in the library and another remaining watch still covers them. Saying the folder has nothing in the
  // library would be false, and the sentence that follows would contradict it.
  it("distinguishes an empty folder from one whose documents another watch still covers", () => {
    expect(describeRemoval(removal(0, 3), english)).toBe(
      "Every document of this folder stays: another watched folder still covers it.",
    );
    expect(describeRemoval(removal(0, 3), spanish)).toBe(
      "Todos los documentos de esta carpeta se quedan: otra carpeta vigilada todavía los cubre.",
    );
    expect(describeRemoval(removal(0, 3), english)).not.toContain("is in your library");
  });

  // "0 documents leave your library" reads as a failure rather than as the answer: when the folder has
  // nothing in the library, the sentence says what is actually happening — only the watch goes.
  it("says so when no document is involved", () => {
    expect(describeRemoval(removal(0), english)).toBe(
      "No document of this folder is in your library, so only the watch is removed.",
    );
    expect(describeRemoval(removal(0), spanish)).toBe(
      "No hay ningún documento de esta carpeta en su biblioteca, así que solo se quita la vigilancia.",
    );
  });
});
