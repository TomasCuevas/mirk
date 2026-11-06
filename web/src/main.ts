import { mount } from "svelte";
import App from "./App.svelte";

/** Mounts the application on the `#app` element. */
export function mountApp(): void {
  // Says which frontend this is before anything can go wrong: a stale bundle looks exactly like a bug.
  console.info(`[mirk] frontend ${__BUILD_STAMP__}`);
  const target = document.getElementById("app");
  if (target === null) {
    throw new Error("the #app mount point is missing from index.html");
  }

  mount(App, { target });
}
