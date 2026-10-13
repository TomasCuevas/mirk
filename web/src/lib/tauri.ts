/**
 * Thin wrapper over Tauri's `invoke`.
 *
 * The import is dynamic so `vite dev` still serves the page in a plain browser, where the module
 * resolves but there is no application to talk to. `runningInTauri` is the check the screen uses to
 * say so honestly instead of rendering an empty library.
 */

/** Whether this page is inside the Tauri webview and can reach the Rust commands. */
export function runningInTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Calls a command exposed by `oct-desktop`, returning its deserialised value. */
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import("@tauri-apps/api/core");
  return api.invoke<T>(command, args);
}
