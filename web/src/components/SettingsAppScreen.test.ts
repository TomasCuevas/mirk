import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsAppScreen from "./SettingsAppScreen.svelte";
import { translate } from "../lib/i18n";
import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";
import type { UpdateState } from "../lib/update";

const english = translate("en");
const spanish = translate("es");

function screenProps(locale: "en" | "es" = "en", update: UpdateState = { kind: "idle" }) {
  return {
    locale,
    t: translate(locale),
    onLocale: () => {},
    onBack: () => {},
    update,
    onCheck: () => {},
    onInstall: () => {},
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

    // R15: the two languages are a segmented control, not a dropdown — both are on screen, and the
    // current one is pressed. Exactly one button may be pressed, and it must be the Spanish one.
    expect(body).toContain('role="group"');
    expect(body).toContain(">English<");
    expect(body).toContain(">Español<");
    expect(body).toMatch(/<button[^>]*aria-pressed="true"[^>]*>Español<\/button>/);
    expect(body).not.toMatch(/<button[^>]*aria-pressed="true"[^>]*>English<\/button>/);
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

/**
 * S7 of `odd/tasks/ci-and-release.md`. The update panel the reader chose instead of a modal: a line of
 * state, the release's own notes, and ONE control whose label says what it will do.
 *
 * What is pinned here is that the control follows the state rather than standing there as a button that
 * may or may not work — the same state can offer "check" or "install" or nothing at all.
 */
describe("the Aplicación screen's update panel", () => {
  const offer = { version: "0.2.0", notes: "Covers are faster.\nAnd the pager is hidden." };

  it("says nothing has been checked yet, and offers to check", () => {
    const { body } = render(SettingsAppScreen, { props: screenProps() });

    expect(body).toContain(english("settings.update.heading"));
    expect(body).toContain(english("settings.update.idle"));
    expect(body).toContain(english("settings.update.hint"));
    expect(body).toContain(english("settings.update.check"));
    // Nothing to install yet, so an install control would be a promise with no version behind it.
    expect(body).not.toContain(english("settings.update.install"));
  });

  it("names the newer version, shows the release's own notes, and offers to install it", () => {
    const { body } = render(SettingsAppScreen, {
      props: screenProps("en", { kind: "available", offer }),
    });

    expect(body).toContain(english("settings.update.available", { version: "0.2.0" }));
    expect(body).toContain("Covers are faster.");
    expect(body).toContain(english("settings.update.install"));
    expect(body).not.toContain(english("settings.update.check"));
  });

  // A release with no description must not leave an empty paragraph behind.
  it("shows no notes block when the release has no notes", () => {
    const { body } = render(SettingsAppScreen, {
      props: screenProps("en", { kind: "available", offer: { version: "0.2.0", notes: null } }),
    });

    expect(body).toContain(english("settings.update.available", { version: "0.2.0" }));
    expect(body).not.toContain('class="notes"');
  });

  it("reports the download instead of offering the install a second time", () => {
    const { body } = render(SettingsAppScreen, {
      props: screenProps("en", { kind: "installing", offer, percent: 42 }),
    });

    expect(body).toContain(english("settings.update.installing", { percent: 42 }));
    // While it runs there is no control at all: a second press would be an install of something already
    // being installed.
    expect(body).not.toContain(english("settings.update.install"));
    expect(body).not.toContain(english("settings.update.check"));
  });

  it("reports a download whose total is unknown without inventing a percentage", () => {
    const { body } = render(SettingsAppScreen, {
      props: screenProps("en", { kind: "installing", offer, percent: null }),
    });

    expect(body).toContain(english("settings.update.installingUnknown"));
    expect(body).not.toContain("%");
  });

  // The distinction the whole panel exists for: "could not ask" is not "up to date".
  it("says a failed check failed, with its reason, and offers to try again", () => {
    const { body } = render(SettingsAppScreen, {
      props: screenProps("en", { kind: "failed", reason: "the endpoint answered 404" }),
    });

    expect(body).toContain(
      english("settings.update.failed", { reason: "the endpoint answered 404" }),
    );
    expect(body).not.toContain(english("settings.update.current"));
    expect(body).toContain(english("settings.update.check"));
  });

  it("says the state in Spanish too", () => {
    const { body } = render(SettingsAppScreen, {
      props: screenProps("es", { kind: "available", offer }),
    });

    expect(body).toContain(spanish("settings.update.heading"));
    expect(body).toContain(spanish("settings.update.available", { version: "0.2.0" }));
    expect(body).toContain(spanish("settings.update.install"));
    expect(body).not.toContain(english("settings.update.install"));
  });
});
