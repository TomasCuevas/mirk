import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsWindowScreen from "./SettingsWindowScreen.svelte";
import { translate } from "../lib/i18n";
import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";
import { layoutLabelKey, windowLayouts, type WindowLayout } from "../lib/window";

const english = translate("en");
const spanish = translate("es");

function screenProps(layout: WindowLayout = "columns-2", locale: "en" | "es" = "en") {
  return {
    layout,
    t: translate(locale),
    onLayout: () => {},
    onBack: () => {},
  };
}

/**
 * S5 of `odd/tasks/window-layout.md`. Ajustes → Ventana: the four layouts the reader can choose.
 *
 * What is pinned here is the whole contract the request states: the four choices are on screen with
 * their words, exactly one of them is marked as the one in force, the screen says what it is for and
 * offers the way back — and the choice is not a second copy of the Ajustes wall, so nothing from
 * another section comes along.
 */
describe("the Ventana sub-screen", () => {
  it("names itself after its section, says what it is for, and offers the way back", () => {
    const { body } = render(SettingsWindowScreen, { props: screenProps() });

    expect(body).toContain(settingsSectionTitle("window", english));
    expect(body).toContain(settingsSectionPurpose("window", english));
    expect(body).toContain(english("settings.back"));
    expect(body).toMatch(/<button[^>]*class="back/);
  });

  it("offers the four layouts by name, in the order the spec names", () => {
    const { body } = render(SettingsWindowScreen, { props: screenProps() });

    expect(body).toContain('role="group"');
    for (const layout of windowLayouts) {
      expect(body, layout).toContain(english(layoutLabelKey(layout)));
    }
    // Four choices and no fifth: the group holds the layouts and nothing else.
    expect((body.match(/aria-pressed=/g) ?? []).length).toBe(windowLayouts.length);
  });

  it("marks exactly the layout in force, and only that one", () => {
    const { body } = render(SettingsWindowScreen, {
      props: screenProps("columns-3"),
    });

    expect(body).toMatch(
      /<button[^>]*aria-pressed="true"[^>]*>3 per row<\/button>/,
    );
    for (const other of windowLayouts.filter((layout) => layout !== "columns-3")) {
      expect(body).not.toMatch(
        new RegExp(`aria-pressed="true"[^>]*>${english(layoutLabelKey(other))}</button>`),
      );
    }
  });

  it("says that the window itself follows the choice", () => {
    const { body } = render(SettingsWindowScreen, { props: screenProps() });

    expect(body).toContain(english("settings.window.hint"));
  });

  it("holds the window choice and nothing from the other sections", () => {
    const { body } = render(SettingsWindowScreen, { props: screenProps() });

    expect(body).toContain(english("settings.window.heading"));
    expect(body).not.toContain(english("settings.language.heading"));
    expect(body).not.toContain(english("settings.folders.heading"));
    expect(body).not.toContain(english("provider.heading"));
    expect(body).not.toContain("<form");
  });

  it("says all of it in Spanish too", () => {
    const { body } = render(SettingsWindowScreen, {
      props: screenProps("auto", "es"),
    });

    expect(body).toContain(settingsSectionTitle("window", spanish));
    expect(body).toContain(settingsSectionPurpose("window", spanish));
    expect(body).toContain(spanish("settings.back"));
    expect(body).toContain(spanish("settings.window.heading"));
    expect(body).toContain(spanish(layoutLabelKey("auto")));
    expect(body).not.toContain(english("settings.window.hint"));
  });
});
