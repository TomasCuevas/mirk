/**
 * The real update channel: the updater plugin, behind the small port `update.ts` describes.
 *
 * Two things live here and nowhere else, because they are the only ones that know the plugin exists:
 *
 * 1. **The dynamic imports.** `@tauri-apps/plugin-updater` and `@tauri-apps/plugin-process` are reached
 *    through `import()`, the same way `tauri.ts` reaches `invoke`, so `vite dev` still serves the page in
 *    a plain browser and nothing resolves an IPC module where there is no application to talk to.
 * 2. **The offer's lifetime.** `check()` hands back a server-side resource, not a value: installing needs
 *    that handle, and the handle has to be released. The offer is remembered here rather than carried
 *    through the interface, where a re-render could drop it and leave an install with nothing to install.
 */
import type { Update } from "@tauri-apps/plugin-updater";
import { percentOf, type UpdateOffer } from "./update";

/** What the interface needs from the update channel, and nothing more. */
export interface UpdatePort {
  /** Asks the manifest for a newer version. Resolves `null` when this build is already the newest. */
  check(): Promise<UpdateOffer | null>;
  /**
   * Downloads and installs the version the last `check()` offered, reporting the downloaded share.
   *
   * `percent` is `null` while the manifest has announced no total; the caller shows the download without
   * a number rather than inventing one.
   */
  install(onProgress: (percent: number | null) => void): Promise<void>;
  /** Enters the installed version. On Linux and macOS the new bundle is on disk but not running. */
  relaunch(): Promise<void>;
}

/** The release body as a sentence, or `null` when the release says nothing. */
function notesOf(body: string | undefined): string | null {
  const trimmed = body?.trim() ?? "";
  return trimmed === "" ? null : trimmed;
}

/** The port backed by the Tauri updater plugin. One per process: it holds the offer. */
export function desktopUpdater(): UpdatePort {
  let offered: Update | null = null;

  return {
    async check() {
      const { check } = await import("@tauri-apps/plugin-updater");
      const found = await check();

      // A check that replaces an unclaimed offer has to release the one it replaced: the handle holds a
      // resource on the Rust side. Best effort — a handle the Rust side already closed is not something
      // the reader can act on, and failing the check over it would hide an answer that did arrive.
      const previous = offered;
      offered = found;
      if (previous !== null && previous !== found) await previous.close().catch(() => {});

      return found === null ? null : { version: found.version, notes: notesOf(found.body) };
    },

    async install(onProgress) {
      const update = offered;
      if (update === null) {
        // Nothing was offered, so there is nothing to install. Said rather than resolved: a success that
        // installed nothing is exactly the silent failure an update channel must never have.
        throw new Error("no update was offered");
      }

      let total: number | null = null;
      let downloaded = 0;

      try {
        await update.downloadAndInstall((event) => {
          if (event.event === "Started") {
            total = event.data.contentLength ?? null;
            // 0% is honest here and only here: the total is known and nothing has arrived yet.
            onProgress(percentOf(0, total));
          } else if (event.event === "Progress") {
            downloaded += event.data.chunkLength;
            onProgress(percentOf(downloaded, total));
          }
          // `Finished` reports nothing the last `Progress` has not already said when a total was
          // announced, and a 100% with no total would be a number nothing measured.
        });
      } catch (error) {
        // The plugin closes the resource when the install completes; when it throws, it may not have.
        await update.close().catch(() => {});
        offered = null;
        throw error;
      }

      // No `close()` here on purpose: on success the Rust side has already released it, and closing a
      // released resource throws. The offer cannot be installed twice either way.
      offered = null;
    },

    async relaunch() {
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    },
  };
}
