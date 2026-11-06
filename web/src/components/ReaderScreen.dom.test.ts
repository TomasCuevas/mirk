import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ReaderScreen from "./ReaderScreen.svelte";
import type { LibraryEntry } from "../lib/library";

/**
 * The screen that opens a document, mounted.
 *
 * What it has to prove is the wiring around `reader-pages` and `reader-source`, because those two are
 * tested where they live: this component is the only place a document is opened for the reader, the
 * only place its failure becomes a sentence and a way out, and the only owner of the loader's lifetime.
 *
 * The page view is replaced by a stub: it needs a real pdf.js document, and what this file asks is not
 * what the page view draws but whether the screen hands it a document at all. `reader-source` is
 * replaced by a promise the test settles, so no file, worker or protocol is involved.
 */

vi.mock("./ReaderPages.svelte", () => ({ default: () => {} }));

/** The IPC surface, so a test can read what the screen asked the application to record. */
const ipc = vi.hoisted(() => ({
  invoke: vi.fn(async (_name: string, _args?: unknown) => null as unknown),
}));
vi.mock("../lib/tauri", () => ({
  invoke: ipc.invoke,
  runningInTauri: () => false,
}));

const source = vi.hoisted(() => ({
  openReaderDocument: vi.fn(),
}));
vi.mock("../lib/reader-source", () => ({
  openReaderDocument: source.openReaderDocument,
}));

/** A library entry: the only thing the open action needs, and one path it must pass through. */
const entry: LibraryEntry = {
  id: 7,
  path: "/home/somebody/Libros/Learning Patterns.pdf",
  title: "Learning Patterns",
  userTitle: null,
  authors: ["Lydia Hallie"],
  readerAuthors: [],
  pages: 436,
  progress: null,
};

/** The catalogue, replaced by the keys themselves: this file asserts which words are chosen, not what. */
const t = (key: string, params?: Record<string, string | number>): string =>
  params === undefined ? key : `${key} ${Object.values(params).join(" ")}`;

let mounted: Record<string, unknown> | null = null;

afterEach(() => {
  if (mounted !== null) unmount(mounted);
  mounted = null;
  source.openReaderDocument.mockReset();
  ipc.invoke.mockClear();
});

function render(): { onClose: () => void; onFallback: () => void; target: HTMLElement } {
  const onClose = vi.fn();
  const onFallback = vi.fn();
  const target = document.createElement("div");
  document.body.append(target);
  mounted = mount(ReaderScreen, { target, props: { entry, t, onClose, onFallback } }) as never;
  flushSync();
  return { onClose, onFallback, target };
}

describe("the reader screen", () => {
  it("asks for the document by the path the entry carries, once", () => {
    source.openReaderDocument.mockReturnValue(new Promise(() => {}));
    render();

    expect(source.openReaderDocument).toHaveBeenCalledTimes(1);
    expect(source.openReaderDocument).toHaveBeenCalledWith(entry.path);
  });

  it("says it is opening until the document is there, and shows the title meanwhile", () => {
    source.openReaderDocument.mockReturnValue(new Promise(() => {}));
    const { target } = render();

    expect(target.textContent).toContain("reader.opening");
    expect(target.textContent).toContain(entry.title);
  });

  it("offers the system viewer, with the reason, when the document cannot be opened", async () => {
    // The failure has to be readable and the reader has to be escapable: a document this reader cannot
    // lay out is not a document the reader may be trapped in.
    source.openReaderDocument.mockRejectedValue(new Error("the asset protocol answered 200"));
    const { target, onFallback } = render();

    await vi.waitFor(() => expect(target.textContent).toContain("reader.failed"));
    expect(target.textContent).toContain("the asset protocol answered 200");

    const fallback = target.querySelector<HTMLButtonElement>(".state button");
    expect(fallback?.textContent).toBe("reader.fallback");
    fallback?.click();
    expect(onFallback).toHaveBeenCalledTimes(1);
  });

  it("writes the reading session, with the page it opened at, when the screen leaves", async () => {
    // R3b: the session is written once, on the way out, because its page visits name it. A reader that
    // opened the document and closed it again has still read for that long, and the visit says where.
    source.openReaderDocument.mockResolvedValue({
      destroy: vi.fn(),
      stats: {},
      document: {},
    });
    const { target } = render();

    // The page view exists only once the document settled, which is when the session begins.
    await vi.waitFor(() => expect(target.querySelector(".pages")).not.toBeNull());

    unmount(mounted as never);
    mounted = null;
    await vi.waitFor(() =>
      expect(ipc.invoke.mock.calls.some(([name]) => name === "reader_record_session")).toBe(true),
    );

    const call = ipc.invoke.mock.calls.find(([name]) => name === "reader_record_session");
    const args = call?.[1] as { id: number; facts: { activeMs: number; visits: { page: number }[] } };
    expect(args.id).toBe(7);
    expect(args.facts.activeMs).toBeGreaterThanOrEqual(0);
    expect(args.facts.visits.map((visit) => visit.page)).toEqual([0]);
  });

  it("closes on its own button", () => {
    source.openReaderDocument.mockReturnValue(new Promise(() => {}));
    const { target, onClose } = render();

    target.querySelector<HTMLButtonElement>(".close")?.click();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("lets the document go when the screen leaves, even if it settles afterwards", async () => {
    // The loader owns a worker and a transport. A screen replaced before the document arrived must not
    // keep them, and the answer that arrives late must be destroyed rather than handed to a dead screen.
    const destroy = vi.fn();
    let settle: (value: unknown) => void = () => {};
    source.openReaderDocument.mockReturnValue(
      new Promise((resolve) => {
        settle = resolve;
      }),
    );

    const { target } = render();
    unmount(mounted as never);
    mounted = null;

    settle({ document: {}, stats: { length: 1, requests: 1, bytes: 1 }, failed: () => null, destroy });
    await vi.waitFor(() => expect(destroy).toHaveBeenCalledTimes(1));
    expect(target.textContent).not.toContain("reader.opening");
  });
});
