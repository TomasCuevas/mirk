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

/**
 * The event the desktop process emits to ask the interface to open a screen (R12).
 *
 * The tray is native and cannot reach a screen by itself, so the request travels as one event whose
 * payload is a screen name. Both halves must spell this name the same way, which is why it lives here
 * beside the only code that listens for it.
 */
export const OPEN_SCREEN_EVENT = "open-screen";

/** Stops listening to an event. */
export type Unlisten = () => void;

/**
 * Calls `handler` for every `event` the desktop process emits, returning the function that stops
 * listening.
 *
 * The dynamic import has the same reason as `invoke`'s: the module resolves in a plain browser, where
 * there is no application to emit anything. Nothing outside the shell may assume a tray exists.
 */
export async function listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  const api = await import("@tauri-apps/api/event");
  return api.listen<T>(event, (received) => handler(received.payload));
}

/** Calls a command exposed by `oct-desktop`, returning its deserialised value. */
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import("@tauri-apps/api/core");
  return api.invoke<T>(command, args);
}
