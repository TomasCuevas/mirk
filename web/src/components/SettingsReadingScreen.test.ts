import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsReadingScreen from "./SettingsReadingScreen.svelte";
import { translate } from "../lib/i18n";
import type { Provider } from "../lib/library";
import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";

const english = translate("en");
const spanish = translate("es");

interface Overrides {
  providers?: Provider[] | null;
  providerError?: string | null;
  spanish?: boolean;
}

function screenProps(overrides: Overrides = {}) {
  return {
    providers: overrides.providers === undefined ? [] : overrides.providers,
    providerError: overrides.providerError ?? null,
    t: translate(overrides.spanish ? "es" : "en"),
    onBack: () => {},
  };
}

const installed: Provider = { id: "evince-gvfs", capability: "live", availability: "installed" };
const missing: Provider = { id: "evince-gvfs", capability: "live", availability: "missing" };

/**
 * R43/S3–S4 (L71). The Lectura sub-screen: the progress providers, explained.
 *
 * Before this round the providers were the second of five flat blocks on one page, with no sentence
 * saying what they had to do with reading. What is pinned here is that they arrived on a screen of their
 * own, that the screen says what it is for, and that all FOUR states the registry keeps apart still
 * reach the screen: still checking, none at all, a provider this machine has, and a provider it does not.
 */
describe("the Lectura sub-screen", () => {
  it("names itself after its section, says what it is for, and offers the way back", () => {
    const { body } = render(SettingsReadingScreen, { props: screenProps() });

    expect(body).toContain(settingsSectionTitle("reading", english));
    expect(body).toContain(settingsSectionPurpose("reading", english));
    // R43/S2: the back control is named after the screen it returns to, not "Back".
    expect(body).toContain(english("settings.back"));
    expect(body).toMatch(/<button[^>]*class="back/);
  });

  // S3: the contents moved and the neighbours did not follow. The language selector is Aplicación's and
  // the folders are Biblioteca's; a sub-screen holding them again would be the wall this round removed.
  it("holds the providers and nothing from the other sections", () => {
    const { body } = render(SettingsReadingScreen, {
      props: screenProps({ providers: [installed] }),
    });

    expect(body).toContain(english("provider.heading"));
    expect(body).not.toContain("<select");
    expect(body).not.toContain(english("settings.folders.heading"));
    expect(body).not.toContain(english("scan.heading"));
    expect(body).not.toContain(english("suggestion.heading"));
  });

  // The registry's three answers are different facts, and the screen keeps them apart: "still checking"
  // may not look like "there are none", which may not look like "this one exists and you cannot use it".
  it("says it is still checking rather than that there are none", () => {
    const { body } = render(SettingsReadingScreen, { props: screenProps({ providers: null }) });

    expect(body).toContain(english("provider.loading"));
    expect(body).not.toContain(english("provider.none"));
  });

  it("says there are none only once the registry says so", () => {
    const { body } = render(SettingsReadingScreen, { props: screenProps({ providers: [] }) });

    expect(body).toContain(english("provider.none"));
  });

  // A provider is named by its readable sentence AND by its stable id, which is what a stored progress
  // row references and what a contributor implements.
  it("states a provider this machine can use, with its stable id", () => {
    const { body } = render(SettingsReadingScreen, {
      props: screenProps({ providers: [installed] }),
    });

    expect(body).toContain("Evince is installed and reports live reading position.");
    expect(body).toContain("evince-gvfs");
  });

  // A provider that is not present is not hidden: the screen says which one is missing and why, which is
  // the sentence that tells a reader what to install rather than that something is broken.
  it("states a provider this machine cannot use, and why", () => {
    const { body } = render(SettingsReadingScreen, {
      props: screenProps({ providers: [missing] }),
    });

    expect(body).toContain("Evince is missing");
    expect(body).toContain("it needs the gio command and an installed Evince or Papers");
  });

  it("reports a registry it could not read, in the screen's own words", () => {
    const { body } = render(SettingsReadingScreen, {
      props: screenProps({ providerError: "no such command" }),
    });

    expect(body).toContain("Could not read the progress providers");
    expect(body).toContain("no such command");
  });

  it("says all of it in Spanish too", () => {
    const { body } = render(SettingsReadingScreen, {
      props: screenProps({ spanish: true, providers: [installed] }),
    });

    expect(body).toContain(settingsSectionTitle("reading", spanish));
    expect(body).toContain(settingsSectionPurpose("reading", spanish));
    expect(body).toContain(spanish("settings.back"));
    expect(body).toContain(spanish("provider.heading"));
  });
});
