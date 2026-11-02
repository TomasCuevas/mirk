import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * S7 of `odd/tasks/ci-and-release.md`: the port that turns the updater plugin into the four things the
 * interface can ask. The plugin is mocked because the alternative is not a test: a real check would need
 * GitHub, a signed release and a new version to find, and the logic worth pinning is the translation —
 * what a null answer means, how chunks become a percentage, what an install with nothing offered does.
 */
const plugin = vi.hoisted(() => ({
  check: vi.fn(),
  downloadAndInstall: vi.fn(),
  close: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({ check: plugin.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: plugin.relaunch }));

const { desktopUpdater } = await import("./updater");

/** The handle the plugin's `check()` returns: a version, a body, and the two methods it carries. */
function offerOf(version: string, body?: string) {
  return { version, body, downloadAndInstall: plugin.downloadAndInstall, close: plugin.close };
}

/** A download that reports exactly these events, then resolves. */
function download(events: readonly Record<string, unknown>[]) {
  return async (onEvent: (event: unknown) => void) => {
    for (const event of events) onEvent(event);
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  plugin.close.mockResolvedValue(undefined);
  plugin.relaunch.mockResolvedValue(undefined);
});

describe("the desktop update port", () => {
  it("reports a newer version with the release's own words, and null when it is current", async () => {
    const port = desktopUpdater();

    plugin.check.mockResolvedValue(null);
    expect(await port.check()).toBeNull();

    plugin.check.mockResolvedValue(offerOf("0.2.0", "Faster covers.\n"));
    expect(await port.check()).toEqual({ version: "0.2.0", notes: "Faster covers." });
  });

  // A release with no description is normal, and the interface must not render an empty paragraph or the
  // word "undefined" where the notes would be.
  it("turns an empty release body into no notes at all", async () => {
    const port = desktopUpdater();
    plugin.check.mockResolvedValue(offerOf("0.2.0", "   \n"));

    expect(await port.check()).toEqual({ version: "0.2.0", notes: null });
  });

  it("reports the download as a share of the announced total", async () => {
    const port = desktopUpdater();
    plugin.check.mockResolvedValue(offerOf("0.2.0"));
    await port.check();

    plugin.downloadAndInstall.mockImplementation(
      download([
        { event: "Started", data: { contentLength: 1000 } },
        { event: "Progress", data: { chunkLength: 400 } },
        { event: "Progress", data: { chunkLength: 600 } },
        { event: "Finished", data: {} },
      ]),
    );

    const percents: (number | null)[] = [];
    await port.install((percent) => percents.push(percent));

    expect(percents).toEqual([0, 40, 100]);
  });

  // A server that sends no content-length is not an error, and an invented 0% would read as "nothing has
  // arrived" for the whole download.
  it("reports no percentage when the total is not announced", async () => {
    const port = desktopUpdater();
    plugin.check.mockResolvedValue(offerOf("0.2.0"));
    await port.check();

    plugin.downloadAndInstall.mockImplementation(
      download([
        { event: "Started", data: {} },
        { event: "Progress", data: { chunkLength: 400 } },
      ]),
    );

    const percents: (number | null)[] = [];
    await port.install((percent) => percents.push(percent));

    expect(percents).toEqual([null, null]);
  });

  // The one refusal this port needs. An install that resolves while installing nothing would leave the
  // interface claiming an update happened, and the relaunch would come back to the same version.
  it("refuses to install when no check offered anything", async () => {
    const port = desktopUpdater();

    await expect(port.install(() => {})).rejects.toThrow("no update was offered");
    expect(plugin.downloadAndInstall).not.toHaveBeenCalled();
  });

  // A failed download must not leave the offer installed-ready: the handle is released and the next check
  // starts from nothing, rather than an install retrying a handle the plugin already gave up on.
  it("releases the offer when the download fails", async () => {
    const port = desktopUpdater();
    plugin.check.mockResolvedValue(offerOf("0.2.0"));
    await port.check();

    plugin.downloadAndInstall.mockRejectedValue(new Error("the connection dropped"));
    await expect(port.install(() => {})).rejects.toThrow("the connection dropped");
    expect(plugin.close).toHaveBeenCalledTimes(1);
  });

  // A second check before the first offer is claimed replaces it, and the replaced handle is released.
  it("releases an unclaimed offer that a later check replaced", async () => {
    const port = desktopUpdater();
    const first = offerOf("0.2.0");
    plugin.check.mockResolvedValueOnce(first);
    await port.check();

    plugin.check.mockResolvedValue(offerOf("0.3.0"));
    await port.check();

    expect(plugin.close).toHaveBeenCalledTimes(1);
  });

  it("enters the installed version through the process plugin", async () => {
    const port = desktopUpdater();

    await port.relaunch();

    expect(plugin.relaunch).toHaveBeenCalledTimes(1);
  });
});
