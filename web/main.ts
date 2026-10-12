/**
 * Frontend shell for the `oct` desktop application.
 *
 * Deliberately framework-free: the UI framework is an open decision, and a scaffold should not make
 * that choice by accident. All this does is prove the Rust bridge works and report what the
 * platform layer resolved to.
 *
 * The Tauri import is dynamic so the same page still runs under plain `vite dev` in a browser,
 * which keeps the frontend iterable without building the whole desktop shell.
 */

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

async function invokeFromTauri(): Promise<Invoke | null> {
  try {
    const api = await import("@tauri-apps/api/core");
    return api.invoke as Invoke;
  } catch {
    return null;
  }
}

function render(id: string, text: string): void {
  const target = document.getElementById(id);
  if (target) {
    target.textContent = text;
  }
}

function renderList(id: string, entries: readonly string[]): void {
  const list = document.getElementById(id);
  if (!list) {
    return;
  }
  list.replaceChildren(
    ...entries.map((entry) => {
      const item = document.createElement("li");
      item.textContent = entry;
      return item;
    }),
  );
}

async function main(): Promise<void> {
  const invoke = await invokeFromTauri();

  if (!invoke) {
    render("platform", "unknown — running outside the Tauri shell");
    renderList("backends", ["unavailable outside the Tauri shell"]);
    return;
  }

  render("platform", await invoke<string>("platform_name"));
  renderList("backends", await invoke<string[]>("backend_ids"));
}

void main();
