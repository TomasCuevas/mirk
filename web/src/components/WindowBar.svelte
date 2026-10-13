<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import { windowControls, type WindowControl } from "../lib/window";

  let {
    t,
    inTauri,
    onAction,
    onDrag,
  }: {
    t: Translate;
    inTauri: boolean;
    onAction: (control: WindowControl) => void;
    onDrag: () => void;
  } = $props();

  // The bar is the window's drag handle. Pressing it (but not one of its buttons) starts the move
  // gesture through an application command. There is deliberately no double-click behaviour: a
  // double-click must not be a second route to maximise (A2), so this handler ignores it.
  function handleMouseDown(event: MouseEvent): void {
    if (!inTauri || event.button !== 0) {
      return;
    }
    if (event.target instanceof HTMLElement && event.target.closest("button") !== null) {
      return;
    }
    onDrag();
  }
</script>

<!--
  The custom top bar (A2). The application is undecorated on purpose, so this is the only title bar
  there is: pressing it moves the window, and it draws exactly the three controls the user asked
  for — minimise, hide and close. There is no maximise control, and the window itself is fixed, so
  there is no other route to the thing the user refused.
-->
<!-- The bar is a drag handle, not a control; its own buttons carry the interactive semantics. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<header onmousedown={handleMouseDown}>
  <span class="brand">{t("app.name")}</span>

  <div class="controls" role="group" aria-label={t("window.controls")}>
    {#each windowControls as control (control.action)}
      <button
        type="button"
        class:close={control.action === "close"}
        title={t(control.labelKey)}
        aria-label={t(control.labelKey)}
        disabled={!inTauri}
        onclick={() => onAction(control)}
      >
        <span aria-hidden="true">{control.glyph}</span>
      </button>
    {/each}
  </div>
</header>

<style>
  header {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    height: 2.5rem;
    padding: 0 0.35rem 0 0.9rem;
    background: #1c2024;
    color: #f6f7f9;
    user-select: none;
  }

  .brand {
    font-size: 0.95rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: lowercase;
  }

  .controls {
    display: flex;
    gap: 0.15rem;
  }

  button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: none;
    border-radius: 0.35rem;
    background: transparent;
    color: #c4cdd8;
    font: inherit;
    font-size: 0.95rem;
    line-height: 1;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
    color: #ffffff;
  }

  button.close:hover:not(:disabled) {
    background: #b53b2a;
    color: #ffffff;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
