import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsAppScreen from "./SettingsAppScreen.svelte";
import { translate } from "../lib/i18n";
import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";

const english = translate("en");
const spanish = translate("es");

function screenProps(locale: "en" | "es" = "en") {
  return {
    locale,
    t: translate(locale),
    onLocale: () => {},
    onBack: () => {},
  };
}

/**
 * R43/S3–S4 (L71). The Aplicación sub-screen: the interface language.
 *
 * This was the first of five flat blocks on the old page — the top of the screen, with nothing above it
 * saying what Ajustes held. It is one setting and one explanation, and both are pinned here: the
 * selector, the language it is currently set to, and the sentence saying that the choice is not private
 * to this window (R1: the desktop process builds the tray's words from the same stored value).
 */
describe("the Aplicación sub-screen", () => {
  it("names itself after its section, says what it is for, and offers the way back", () => {
    const { body } = render(SettingsAppScreen, { props: screenProps() });

    expect(body).toContain(settingsSectionTitle("app", english));
    expect(body).toContain(settingsSectionPurpose("app", english));
    // R43/S2: the back control is named after the screen it returns to, not "Back".
    expect(body).toContain(english("settings.back"));
    expect(body).toMatch(/<button[^>]*class="back/);
  });

  it("offers both languages, and marks the one the interface is in", () => {
    const { body } = render(SettingsAppScreen, { props: screenProps("es") });

    expect(body).toContain("<select");
    // Both languages are offered, and the only one that can be marked as chosen is the current one:
    // the selector states the choice so a reader sees what it is set to before touching it. Asserting
    // the bare option tags for BOTH would contradict the marking below — the first version of this
    // test did exactly that and failed as soon as the screen marked the current language.
    expect(body).toContain(">English<");
    expect(body).toContain(">Español<");
    expect(body).toMatch(/<option value="es"[^>]*selected[^>]*>Español<\/option>/);
    expect(body).not.toMatch(/<option value="en"[^>]*selected[^>]*>English<\/option>/);
  });

  // R1: the tray's menu is built by the desktop process from the same stored value, so the screen says
  // the choice is not private to this window. It is the one sentence the old block never had.
  it("says the choice reaches the tray and the next launch", () => {
    const { body } = render(SettingsAppScreen, { props: screenProps() });

    expect(body).toContain(english("settings.language.hint"));
  });

  // S3: nothing from the other two sections came along. A sub-screen is one kind of setting.
  it("holds the language and nothing from the other sections", () => {
    const { body } = render(SettingsAppScreen, { props: screenProps() });

    expect(body).toContain(english("settings.language.heading"));
    expect(body).not.toContain(english("provider.heading"));
    expect(body).not.toContain(english("settings.folders.heading"));
    expect(body).not.toContain(english("scan.heading"));
    expect(body).not.toContain("<form");
  });

  it("says all of it in Spanish too", () => {
    const { body } = render(SettingsAppScreen, { props: screenProps("es") });

    expect(body).toContain(settingsSectionTitle("app", spanish));
    expect(body).toContain(settingsSectionPurpose("app", spanish));
    expect(body).toContain(spanish("settings.back"));
    expect(body).toContain(spanish("settings.language.heading"));
    // The catalogue's hint is the Spanish one, not the English string left behind.
    expect(body).not.toContain(english("settings.language.hint"));
  });
});
