import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import SettingsLibraryScreen from "./SettingsLibraryScreen.svelte";
import { translate } from "../lib/i18n";
import {
  settingsSectionPurpose,
  settingsSectionTitle,
  type FolderRefusal,
  type FolderRemoval,
  type WatchedFolder,
} from "../lib/screens";

const english = translate("en");
const spanish = translate("es");

interface Overrides {
  inTauri?: boolean;
  spanish?: boolean;
  folders?: WatchedFolder[] | null;
  removal?: FolderRemoval | null;
  refusal?: FolderRefusal | null;
}

/**
 * The Biblioteca sub-screen's props (R43/S3).
 *
 * These used to be one long Ajustes screen's props. The contents moved, so the props moved with them,
 * and nothing else changed: the same defaults, the same `null` meaning "the store has not answered".
 */
function screenProps(overrides: Overrides = {}) {
  const isSpanish = overrides.spanish ?? false;
  return {
    folders: overrides.folders ?? [],
    foldersError: null,
    t: translate(isSpanish ? "es" : "en"),
    scanning: false,
    scanReport: null,
    scanError: null,
    onScan: async () => {},
    onOpenFolder: () => {},
    removal: overrides.removal ?? null,
    onAskRemoval: () => {},
    onConfirmRemoval: () => {},
    onCancelRemoval: () => {},
    refusal: overrides.refusal ?? null,
    onCloseRefusal: () => {},
    inTauri: overrides.inTauri ?? true,
    onBack: () => {},
  };
}

/**
 * R43/S1–S4 (L71). The sub-screen explains itself, and only holds its own blocks.
 *
 * The index answers "what is in there" for someone choosing; this screen answers "what is this for" for
 * someone who has already chosen, and offers the way back to the index it belongs to. Both sentences are
 * pinned here because they are the whole point of the round: the user's words were that Ajustes is
 * *"nada intuitiva"*, and a sub-screen that opens with five unlabelled blocks would repeat it.
 */
describe("the Biblioteca sub-screen explains itself", () => {
  it("names itself after its section, says what it is for, and says where the way back goes", () => {
    const { body } = render(SettingsLibraryScreen, { props: screenProps() });

    expect(body).toContain(settingsSectionTitle("library", english));
    expect(body).toContain(settingsSectionPurpose("library", english));
    // R43/S2: the back control is named after the screen it returns to, not "Back".
    expect(body).toContain(english("settings.back"));
    // ...and it is a control, not a label.
    expect(body).toMatch(/<button[^>]*class="back/);
  });

  // S3: the contents moved, they did not follow. A sub-screen that still carried the language selector
  // and the providers would be the old wall with a heading on it.
  //
  // S9 of `odd/tasks/suggestions-screen.md`: and the queue is NOT one of its blocks any more. The screen
  // of its own is the only place it is read or decided, so the assertion is about its absence — the
  // heading, the definition line, the cards and their two decisions all left together.
  it("holds the folders and the scan, and not the queue or the other sections' blocks", () => {
    const { body } = render(SettingsLibraryScreen, { props: screenProps() });

    expect(body).toContain(english("settings.folders.heading"));
    expect(body).toContain(english("scan.heading"));
    expect(body).not.toContain(english("suggestion.heading"));
    // The cards are gone, which is what "the queue left" looks like from the markup: no cover space with
    // its two decisions, and no path control that would open a suggested document's folder.
    expect(body).not.toContain("cover-decisions");
    expect(body).not.toContain(english("suggestion.openFolder", { path: "/elsewhere/x.pdf" }));
    expect(body).not.toContain(english("provider.heading"));
    expect(body).not.toContain("<select");
  });

  it("says all of it in Spanish too", () => {
    const { body } = render(SettingsLibraryScreen, { props: screenProps({ spanish: true }) });

    expect(body).toContain(settingsSectionTitle("library", spanish));
    expect(body).toContain(settingsSectionPurpose("library", spanish));
    expect(body).toContain(spanish("settings.back"));
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
    const { body } = render(SettingsLibraryScreen, { props: screenProps() });

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
    const { body } = render(SettingsLibraryScreen, { props: screenProps({ inTauri: false }) });

    expect(body).not.toContain("Choose folder");
    expect(body).toContain("Enter the path by hand");
  });

  it("says both things in Spanish too", () => {
    const { body } = render(SettingsLibraryScreen, { props: screenProps({ spanish: true }) });

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
    const { body } = render(SettingsLibraryScreen, { props: screenProps() });

    expect(body).toContain("No folder chosen yet.");
    expect(body).toContain('class="field');
  });

  it("puts the two actions in one set, and refuses to scan without a path", () => {
    const { body } = render(SettingsLibraryScreen, { props: screenProps() });

    expect(body).toContain('class="actions');
    // Nothing to scan: the button is there but not usable, rather than absent — a disabled button states
    // what the panel is for, which a missing one does not.
    expect(body).toMatch(/<button[^>]*type="submit"[^>]*disabled/);
  });

  it("keeps the manual route as a quiet link and drops the browser's triangle", () => {
    const { body } = render(SettingsLibraryScreen, { props: screenProps() });

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
    const { body } = render(SettingsLibraryScreen, {
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
    const { body } = render(SettingsLibraryScreen, {
      props: screenProps({ refusal: { covered: watched(), candidate: "/lib/pdfs" } }),
    });

    expect(body).toContain("That folder is already watched, so it was not added.");
    expect(body).not.toContain("inside /lib/pdfs");
  });

  it("says it in Spanish too", () => {
    const { body } = render(SettingsLibraryScreen, {
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
    const { body } = render(SettingsLibraryScreen, { props: screenProps({ removal: removal(1) }) });

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
    const { body } = render(SettingsLibraryScreen, { props: screenProps({ removal: removal(4) }) });

    expect(body).toContain("4 documents of this folder leave your library.");
  });

  // The guard has to be visible where it matters: a document another watch still covers is kept, and a
  // reader who expected their whole library to empty should be told that before confirming.
  it("says when some documents stay, and stays quiet when none do", () => {
    const withKept = render(SettingsLibraryScreen, {
      props: screenProps({ removal: removal(2, 3) }),
    });
    expect(withKept.body).toContain(
      "Documents that another watched folder still covers stay.",
    );

    const withoutKept = render(SettingsLibraryScreen, { props: screenProps({ removal: removal(2) }) });
    expect(withoutKept.body).not.toContain(
      "Documents that another watched folder still covers stay.",
    );
  });

  // Removing a nested watch takes nothing, because the outer watch still finds every file: the
  // confirmation must say THAT, not that the folder has nothing in the library, which is false.
  it("says every document stays when a remaining watch still covers them all", () => {
    const { body } = render(SettingsLibraryScreen, {
      props: screenProps({ removal: removal(0, 3) }),
    });

    expect(body).toContain(
      "Every document of this folder stays: another watched folder still covers it.",
    );
    expect(body).not.toContain("is in your library");
    // ...and the follow-up line is not repeated, because the sentence above already said it.
    expect(body).not.toContain("Documents that another watched folder still covers stay.");
  });

  it("says it in Spanish too", () => {
    const { body } = render(SettingsLibraryScreen, {
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
    const { body } = render(SettingsLibraryScreen, {
      props: screenProps({ folders: [watched()] }),
    });

    expect(body).toContain("Stop watching /lib/pdfs and remove its documents");
  });

  it("says it in Spanish too", () => {
    const { body } = render(SettingsLibraryScreen, {
      props: screenProps({ spanish: true, folders: [watched()] }),
    });

    expect(body).toContain("Dejar de vigilar /lib/pdfs y quitar sus documentos");
  });
});
