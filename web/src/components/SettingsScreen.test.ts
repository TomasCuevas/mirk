import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsScreen from "./SettingsScreen.svelte";
import { translate } from "../lib/i18n";
import type { FolderRefusal, FolderRemoval, WatchedFolder } from "../lib/screens";
import type { Suggestion } from "../lib/suggestions";

interface Overrides {
  inTauri?: boolean;
  spanish?: boolean;
  folders?: WatchedFolder[] | null;
  removal?: FolderRemoval | null;
  refusal?: FolderRefusal | null;
  suggestions?: Suggestion[] | null;
  suggestionError?: string | null;
  deciding?: boolean;
}

function screenProps(overrides: Overrides = {}) {
  const spanish = overrides.spanish ?? false;
  return {
    providers: [],
    providerError: null,
    folders: overrides.folders ?? [],
    foldersError: null,
    locale: spanish ? ("es" as const) : ("en" as const),
    t: translate(spanish ? "es" : "en"),
    scanning: false,
    scanReport: null,
    scanError: null,
    onScan: async () => {},
    onLocale: () => {},
    onOpenFolder: () => {},
    removal: overrides.removal ?? null,
    onAskRemoval: () => {},
    onConfirmRemoval: () => {},
    onCancelRemoval: () => {},
    refusal: overrides.refusal ?? null,
    onCloseRefusal: () => {},
    inTauri: overrides.inTauri ?? true,
    suggestions: overrides.suggestions === undefined ? [] : overrides.suggestions,
    suggestionError: overrides.suggestionError ?? null,
    deciding: overrides.deciding ?? false,
    onDecide: async () => {},
    onOpenDocument: () => {},
  };
}

const suggestion = (overrides: Partial<Suggestion> = {}): Suggestion => ({
  id: 1,
  path: "/elsewhere/Cosmos - Carl Sagan.pdf",
  seenCount: 3,
  firstSeenAt: 1776000000,
  ...overrides,
});

/**
 * L56. The queue of documents oct saw open, and the only place the reader can decide about them.
 *
 * These tests exist because a rule that is computed and then never wired is this project's recurring
 * failure: the pure decisions have their own tests in \`suggestions.test.ts\`, and what is checked here is
 * that the section actually reaches the screen — with both ways out, with an honest failure, and with
 * the three states kept apart.
 */
describe("the suggestions the reader decides about", () => {
  it("lists a document waiting for a decision, with both ways out", () => {
    const { body } = render(
      SettingsScreen,
      { props: screenProps({ suggestions: [suggestion()] }) },
    );

    // L57: the card reads like a library card — the file's own name as the heading, the path under it.
    expect(body).toContain("Cosmos - Carl Sagan");
    expect(body).toContain("/elsewhere/Cosmos - Carl Sagan.pdf");
    // L58: and the path is a control, not a dead line: it opens the folder that holds the document.
    expect(body).toContain("Open the folder that holds /elsewhere/Cosmos - Carl Sagan.pdf");
    // L59: the user does not want the observation count on the card at all — it was this project's
    // addition, not their request, and it is gone: no sentence, no keys, no helper.
    expect(body).not.toContain("Opened");
    expect(body).not.toContain("Se abrió");
    // Both decisions, as icons: the drawing is hidden from assistive technology and the name comes
    // from the catalogue, exactly as the card's own icon actions work.
    expect(body).toContain("Add /elsewhere/Cosmos - Carl Sagan.pdf to the library");
    expect(body).toContain("Discard the suggestion for /elsewhere/Cosmos - Carl Sagan.pdf");
    expect(body).toContain("cover-decisions");
    expect(body).toContain("<svg");
  });

  // The heading is the file's own name, and the claim stops there: nothing on the card says the
  // document has a title, because oct has not read this file — it only saw it open.
  it("does not claim a title it never read", () => {
    const { body } = render(
      SettingsScreen,
      { props: screenProps({ suggestions: [suggestion()] }) },
    );

    // The heading carries the name without the extension, and the path under it is the one place the
    // extension belongs — which is why the claim is made about the heading and not about the page.
    expect(body).toContain(">Cosmos - Carl Sagan</h3>");
    expect(body).not.toContain(">Cosmos - Carl Sagan.pdf</h3>");
  });

  // The decision is per document and it is explicit: the control names the path it acts on, so no
  // button on this screen can be read as "add whatever is pending".
  it("names the document each decision acts on", () => {
    const { body } = render(
      SettingsScreen,
      { props: screenProps({ suggestions: [suggestion({ path: "/elsewhere/Otro.pdf" })] }) },
    );

    expect(body).toContain("Add /elsewhere/Otro.pdf to the library");
  });

  // The three states, and the difference between the first two is the point: "still checking" and
  // "nothing is waiting" are different facts, and an empty queue that merely looks like good news is
  // exactly what the producer's own doc comment refuses.
  it("says it is still checking rather than that there is nothing to decide", () => {
    const { body } = render(
      SettingsScreen,
      { props: screenProps({ suggestions: null }) },
    );

    expect(body).toContain("Checking for documents opened outside oct");
    expect(body).not.toContain("No document is waiting for a decision");
  });

  it("says nothing is waiting only once the store says so", () => {
    const { body } = render(SettingsScreen, { props: screenProps({ suggestions: [] }) });

    expect(body).toContain("No document is waiting for a decision");
    // Nothing to decide means nothing to press: no card, and therefore no decisions on it.
    expect(body).not.toContain("cover-decisions");
  });

  it("reports a decision that could not be recorded, in the section's own words", () => {
    const { body } = render(
      SettingsScreen,
      {
        props: screenProps({
          suggestions: [suggestion()],
          suggestionError: "the file is gone",
        }),
      },
    );

    expect(body).toContain("The decision could not be recorded");
    expect(body).toContain("the file is gone");
  });

  it("speaks Spanish in the section too, because both catalogues carry its words", () => {
    const { body } = render(
      SettingsScreen,
      { props: screenProps({ spanish: true, suggestions: [suggestion()] }) },
    );

    expect(body).toContain("Sugerencias");
    expect(body).toContain("Agregar /elsewhere/Cosmos - Carl Sagan.pdf a la biblioteca");
  });
});

const watched = (path = "/lib/pdfs"): WatchedFolder => ({
  id: 1,
  path,
  recursive: true,
  enabled: true,
});

/**
 * R11 (L22). The chooser is the primary route and the pasted path is the secondary one, and neither is
 * a detail a refactor should be free to drop — which is exactly the failure this file exists to catch,
 * since a rule with no test can be computed and then never wired without anything going red.
 */
describe("the folder chooser", () => {
  it("offers the system folder browser, with manual entry kept and collapsed (R11)", () => {
    const { body } = render(SettingsScreen, { props: screenProps() });

    expect(body).toContain("Choose folder");
    // Manual entry still exists...
    expect(body).toContain("Enter the path by hand");
    // ...but collapsed: a `<details>` without `open` is closed until the person asks for it.
    expect(body).toContain("<details");
    expect(body).not.toContain("<details open");
  });

  // Outside the desktop shell there is no dialog to open, so offering the button would be a promise
  // that cannot be kept. Typing a path is still possible, because that route needs nothing from the
  // application.
  it("does not offer a dialog outside the shell, and keeps the manual route", () => {
    const { body } = render(SettingsScreen, { props: screenProps({ inTauri: false }) });

    expect(body).not.toContain("Choose folder");
    expect(body).toContain("Enter the path by hand");
  });

  it("says both things in Spanish too", () => {
    const { body } = render(SettingsScreen, { props: screenProps({ spanish: true }) });

    expect(body).toContain("Elegir carpeta");
    expect(body).toContain("Escribir la ruta a mano");
  });
});

/**
 * R25 (L42). The section the user called ugly was a stack of six blocks of equal weight. What this
 * pins is the composition it was rebuilt into: the folder is the subject and reads as a readout, the
 * two actions sit together in one set, and the manual route is a quiet link rather than a box.
 */
describe("the scan section's composition", () => {
  it("shows the folder as a readout, and says so when there is none", () => {
    const { body } = render(SettingsScreen, { props: screenProps() });

    expect(body).toContain("No folder chosen yet.");
    expect(body).toContain('class="field');
  });

  it("puts the two actions in one set, and refuses to scan without a path", () => {
    const { body } = render(SettingsScreen, { props: screenProps() });

    expect(body).toContain('class="actions');
    // Nothing to scan: the button is there but not usable, rather than absent — a disabled button states
    // what the panel is for, which a missing one does not.
    expect(body).toMatch(/<button[^>]*type="submit"[^>]*disabled/);
  });

  it("keeps the manual route as a quiet link and drops the browser's triangle", () => {
    const { body } = render(SettingsScreen, { props: screenProps() });

    expect(body).toContain("Enter the path by hand");
    // The disclosure is styled into a link, which means hiding the marker the browser draws for it.
    expect(body).toContain("summary");
  });
});

/**
 * R26 (L42). The refusal was a line under the form and is a modal now. A test can see this because both
 * modals read from props: a rule about how something is presented can only fail if something is looking.
 */
describe("the refusal, as a modal", () => {
  it("speaks inside a dialog rather than under the form", () => {
    const { body } = render(SettingsScreen, {
      props: screenProps({
        refusal: { covered: watched(), candidate: "/lib/pdfs/novelas" },
      }),
    });

    expect(body).toContain('role="dialog"');
    expect(body).toContain(
      "That folder is inside /lib/pdfs, which is already watched, so it was not added.",
    );
    // The sentence is inside the modal, not somewhere below the form: the dialog opens before it.
    expect(body.indexOf('role="dialog"')).toBeLessThan(body.indexOf("which is already watched"));
  });

  it("says a folder is already watched when it is the covering folder itself", () => {
    const { body } = render(SettingsScreen, {
      props: screenProps({ refusal: { covered: watched(), candidate: "/lib/pdfs" } }),
    });

    expect(body).toContain("That folder is already watched, so it was not added.");
    expect(body).not.toContain("inside /lib/pdfs");
  });

  it("says it in Spanish too", () => {
    const { body } = render(SettingsScreen, {
      props: screenProps({
        spanish: true,
        refusal: { covered: watched(), candidate: "/lib/pdfs" },
      }),
    });

    expect(body).toContain("Esa carpeta no se agregó");
    expect(body).toContain("Esa carpeta ya está vigilada, así que no se agregó.");
    expect(body).toContain("Entendido");
  });
});

/**
 * R27 (L42). Removing a watch now takes its documents out of the library, so it asks first — and the
 * confirmation has to state what it will take, because that is the whole reason for asking.
 */
describe("the removal's confirmation", () => {
  const removal = (documents: number, kept = 0): FolderRemoval => ({
    folder: watched(),
    documents: Array.from({ length: documents }, (_, index) => `/lib/pdfs/${index}.pdf`),
    kept: Array.from({ length: kept }, (_, index) => `/lib/inner/${index}.pdf`),
  });

  it("names the folder and says how many documents leave", () => {
    const { body } = render(SettingsScreen, { props: screenProps({ removal: removal(1) }) });

    expect(body).toContain('role="dialog"');
    expect(body).toContain("Stop watching this folder?");
    expect(body).toContain("/lib/pdfs");
    expect(body).toContain("1 document of this folder leaves your library.");
    // The reason the confirmation is not merely frightening: the positions are recoverable.
    expect(body).toContain("The reading position is kept by the viewer");
    expect(body).toContain("Remove folder and documents");
    expect(body).toContain("Cancel");
  });

  it("counts more than one", () => {
    const { body } = render(SettingsScreen, { props: screenProps({ removal: removal(4) }) });

    expect(body).toContain("4 documents of this folder leave your library.");
  });

  // The guard has to be visible where it matters: a document another watch still covers is kept, and a
  // reader who expected their whole library to empty should be told that before confirming.
  it("says when some documents stay, and stays quiet when none do", () => {
    const withKept = render(SettingsScreen, { props: screenProps({ removal: removal(2, 3) }) });
    expect(withKept.body).toContain(
      "Documents that another watched folder still covers stay.",
    );

    const withoutKept = render(SettingsScreen, { props: screenProps({ removal: removal(2) }) });
    expect(withoutKept.body).not.toContain(
      "Documents that another watched folder still covers stay.",
    );
  });

  // Removing a nested watch takes nothing, because the outer watch still finds every file: the
  // confirmation must say THAT, not that the folder has nothing in the library, which is false.
  it("says every document stays when a remaining watch still covers them all", () => {
    const { body } = render(SettingsScreen, { props: screenProps({ removal: removal(0, 3) }) });

    expect(body).toContain(
      "Every document of this folder stays: another watched folder still covers it.",
    );
    expect(body).not.toContain("is in your library");
    // ...and the follow-up line is not repeated, because the sentence above already said it.
    expect(body).not.toContain("Documents that another watched folder still covers stay.");
  });

  it("says it in Spanish too", () => {
    const { body } = render(SettingsScreen, {
      props: screenProps({ spanish: true, removal: removal(1) }),
    });

    expect(body).toContain("¿Dejar de vigilar esta carpeta?");
    expect(body).toContain("1 documento de esta carpeta sale de su biblioteca.");
    expect(body).toContain("Quitar la carpeta y sus documentos");
  });
});

/**
 * R27 also changes what the folder row's own control means, and its accessible name is where that is
 * recorded for someone who cannot see the confirmation it opens.
 */
describe("the folder row", () => {
  it("describes the removal as taking the documents with it", () => {
    const { body } = render(SettingsScreen, {
      props: screenProps({ folders: [watched()] }),
    });

    expect(body).toContain("Stop watching /lib/pdfs and remove its documents");
  });

  it("says it in Spanish too", () => {
    const { body } = render(SettingsScreen, {
      props: screenProps({ spanish: true, folders: [watched()] }),
    });

    expect(body).toContain("Dejar de vigilar /lib/pdfs y quitar sus documentos");
  });
});
