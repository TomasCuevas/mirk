import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import Navbar from "./Navbar.svelte";
import { translate } from "../lib/i18n";
import { screenLabel, screens, type Screen } from "../lib/screens";

const english = translate("en");
const spanish = translate("es");

function navProps(mode: "bottom" | "side" = "bottom", screen: Screen = "home") {
  return {
    screen,
    mode,
    t: english,
    onSelect: () => {},
  };
}

/**
 * S2–S4 of `odd/tasks/navigation.md`. The navigation is the same three destinations in two
 * orientations: the taller bottom bar, and the full screen's left sidebar.
 *
 * What is pinned here is what a reader gets: every destination with its label and its icon, exactly one
 * marked as the current screen, and the orientation the layout asked for. The icons are decorative
 * (`aria-hidden`), so the label stays the one accessible name and nothing is announced twice.
 */
describe("the navigation", () => {
  it("draws every destination with its label and its icon", () => {
    const { body } = render(Navbar, { props: navProps() });

    for (const destination of screens) {
      expect(body).toContain(screenLabel(destination, english));
    }
    expect((body.match(/<svg/g) ?? []).length).toBe(screens.length);
    // The drawings are decoration: the button's text is its name.
    for (const icon of body.match(/<svg[^>]*>/g) ?? []) {
      expect(icon).toContain('aria-hidden="true"');
    }
    expect(body).toContain(english("nav.ariaLabel"));
  });

  it("marks exactly the screen the reader is on", () => {
    const { body } = render(Navbar, { props: navProps("bottom", "library") });

    expect(body).toMatch(/<button[^>]*aria-current="page"[^>]*>[\s\S]*?Library[\s\S]*?<\/button>/);
    expect((body.match(/aria-current="page"/g) ?? []).length).toBe(1);
  });

  it("stands at the bottom of a window", () => {
    const { body } = render(Navbar, { props: navProps("bottom") });

    expect(body).toMatch(/<nav[^>]*class="bottom(?:\s|")/);
  });

  // S3: full screen has the room for a sidebar, so the same three destinations stand on the left.
  it("moves the same three destinations into the sidebar in full screen", () => {
    const { body } = render(Navbar, { props: navProps("side") });

    expect(body).toMatch(/<nav[^>]*class="side(?:\s|")/);
    for (const destination of screens) {
      expect(body).toContain(screenLabel(destination, english));
    }
    expect((body.match(/<svg/g) ?? []).length).toBe(screens.length);
  });

  it("says it in Spanish too, in both orientations", () => {
    for (const mode of ["bottom", "side"] as const) {
      const { body } = render(Navbar, {
        props: { ...navProps(mode), t: spanish },
      });
      for (const destination of screens) {
        expect(body).toContain(screenLabel(destination, spanish));
      }
      expect(body).toContain(spanish("nav.ariaLabel"));
    }
  });
});
