import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsScreen from "./SettingsScreen.svelte";
import { translate } from "../lib/i18n";
import {
  settingsSectionSummary,
  settingsSectionTitle,
  settingsSections,
} from "../lib/screens";

const english = translate("en");
const spanish = translate("es");

function indexProps(locale: "en" | "es" = "en", updateAvailable = false) {
  return {
    t: translate(locale),
    onOpenSection: () => {},
    updateAvailable,
  };
}

/**
 * R43/S1 (L71). Ajustes is an index, not a wall.
 *
 * The user's report was *"no es nada intuitiva actualmente"*: five flat blocks — Language, Providers,
 * Watched folders, Suggestions, Scan — of equal weight, and nothing saying which one holds what. What is
 * pinned here is the shape they chose when asked: one row per section, each with its name and ONE plain sentence,
 * and none of the five blocks left behind on this page.
 *
 * The sub-screens' own tests live beside them (`SettingsLibraryScreen.test.ts`,
 * `SettingsReadingScreen.test.ts`, `SettingsAppScreen.test.ts`), which is where every assertion that used
 * to be in this file went: they moved with the contents rather than being rewritten.
 */
describe("Ajustes as an index", () => {
  it("shows four rows, each with its name and one sentence about what lives inside", () => {
    const { body } = render(SettingsScreen, { props: indexProps() });

    for (const section of settingsSections) {
      expect(body).toContain(settingsSectionTitle(section, english));
      expect(body).toContain(settingsSectionSummary(section, english));
    }
    // Exactly four choices, one per section: the index renders `settingsSections` and nothing else.
    expect((body.match(/<button/g) ?? []).length).toBe(settingsSections.length);
  });

  // The point of the round: the five blocks are moved, not duplicated. A page that still held the folder
  // list under three rows would be the same wall with a table of contents on top.
  it("leaves the five blocks to their sub-screens instead of stacking them here", () => {
    const { body } = render(SettingsScreen, { props: indexProps() });

    expect(body).not.toContain("<select");
    expect(body).not.toContain("<form");
    expect(body).not.toContain(english("settings.folders.heading"));
    expect(body).not.toContain(english("provider.heading"));
    expect(body).not.toContain(english("suggestion.heading"));
    expect(body).not.toContain(english("scan.heading"));
  });

  // R3: the navbar and the screen agree on the word, and the index says what it is an index OF.
  it("names the screen and explains the grouping in one line", () => {
    const { body } = render(SettingsScreen, { props: indexProps() });

    expect(body).toContain(english("settings.heading"));
    expect(body).toContain(english("settings.intro"));
  });

  it("says it in Spanish too, because both catalogues carry the index's words", () => {
    const { body } = render(SettingsScreen, { props: indexProps("es") });

    for (const section of settingsSections) {
      expect(body).toContain(settingsSectionTitle(section, spanish));
      expect(body).toContain(settingsSectionSummary(section, spanish));
    }
    expect(body).toContain(spanish("settings.intro"));
  });
});

/**
 * S3 of `odd/tasks/desktop-chrome.md`: every row of the index has a drawing, and the drawings are
 * decoration — the row's name remains the accessible content, so they are hidden from assistive
 * technology.
 */
describe("the index's icons", () => {
  it("gives every section exactly one decorative drawing", () => {
    const { body } = render(SettingsScreen, { props: indexProps() });

    expect((body.match(/<svg/g) ?? []).length).toBe(settingsSections.length);
    for (const icon of body.match(/<svg[^>]*>/g) ?? []) {
      expect(icon).toContain('aria-hidden="true"');
    }
  });
});

/**
 * S7 of `odd/tasks/ci-and-release.md`. The reader chose a discreet notice over a modal: when a newer
 * version is waiting, the index marks the row that leads to it and says nothing anywhere else.
 *
 * The mark is announced rather than decorative — a dot that exists only visually carries no message to a
 * screen reader — and it is pinned to ONE row, because a mark on the wrong section sends the reader to a
 * screen that has nothing to install.
 */
describe("the update mark", () => {
  it("marks the Aplicación row only while a newer version is waiting", () => {
    const marked = render(SettingsScreen, { props: indexProps("en", true) }).body;

    expect(marked).toContain(`aria-label="${english("settings.update.dot")}"`);
    // Svelte appends its own scoping class, so the attribute is matched by what it contains.
    expect((marked.match(/class="[^"]*\bwaiting\b/g) ?? []).length).toBe(1);

    const clean = render(SettingsScreen, { props: indexProps("en", false) }).body;
    expect(clean).not.toMatch(/class="[^"]*\bwaiting\b/);
  });

  it("marks the row that holds the update, and no other", () => {
    const { body } = render(SettingsScreen, { props: indexProps("en", true) });

    const appRow = body.slice(body.indexOf(settingsSectionTitle("app", english)));
    expect(appRow.search(/class="[^"]*\bwaiting\b/)).toBeGreaterThanOrEqual(0);
    expect(appRow.search(/class="[^"]*\bwaiting\b/)).toBeLessThan(
      appRow.indexOf(settingsSectionSummary("app", english)),
    );
  });

  it("announces the mark in Spanish", () => {
    const { body } = render(SettingsScreen, { props: indexProps("es", true) });

    expect(body).toContain(`aria-label="${spanish("settings.update.dot")}"`);
  });
});
