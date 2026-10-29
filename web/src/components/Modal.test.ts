import { render } from "svelte/server";
import type { Snippet } from "svelte";
import { describe, expect, it } from "vitest";
import Modal from "./Modal.svelte";

/** A modal's body snippet, which is what a caller hands it. */
const body = (() => {}) as unknown as Snippet;

function modalProps(size?: "narrow" | "wide") {
  return {
    title: "A dialog",
    onClose: () => {},
    children: body,
    ...(size === undefined ? {} : { size }),
  };
}

/**
 * L13: the dialog's width follows its use, not only the window.
 *
 * A removal confirmation is one sentence, and a wide box around one sentence is the same complaint as a
 * narrow editor in a wide window. The editor is the case with content to justify the room, so it asks
 * for the wide dialog and the confirmations keep the narrow one by default.
 */
describe("the dialog's width", () => {
  it("is narrow unless the caller asks otherwise", () => {
    const { body: narrow } = render(Modal, { props: modalProps() });
    expect(narrow).not.toMatch(/class="[^"]*\bwide\b/);
    expect(narrow).toContain("A dialog");
  });

  it("grows when the caller has content for the room", () => {
    const { body: wide } = render(Modal, { props: modalProps("wide") });
    expect(wide).toMatch(/class="[^"]*\bwide\b/);
  });
});
