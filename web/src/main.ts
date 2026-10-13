import { mount } from "svelte";
import App from "./App.svelte";

/** Mounts the application on the `#app` element. */
export function mountApp(): void {
  const target = document.getElementById("app");
  if (target === null) {
    throw new Error("the #app mount point is missing from index.html");
  }

  mount(App, { target });
}
