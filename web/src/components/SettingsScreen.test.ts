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

function indexProps(locale: "en" | "es" = "en") {
  return {
    t: translate(locale),
    onOpenSection: () => {},
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
