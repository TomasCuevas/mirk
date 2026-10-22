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
  for — minimise, hide and close. There is no maximise control: the window's size is a stored choice
  in Ajustes → Ventana, so nothing here can grow it behind the reader's back — and it stays visible in
  full screen, which is what keeps closing and hiding reachable there.
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
        <svg
          viewBox="0 0 24 24"
          width="16"
          height="16"
          fill="none"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
          focusable="false"
        >
          {#if control.icon === "minimize"}
            <path d="M6 12h12" />
          {:else if control.icon === "hide"}
            <!-- R16: the first drawing was an arrow into a tray, and the user read it as "download" —
                 which it is, in every icon set. Hiding a window is hiding it, so this is an eye with
                 a slash: the one metaphor that cannot be mistaken for a transfer. -->
            <path d="M4.2 12s3-5.2 7.8-5.2S19.8 12 19.8 12s-3 5.2-7.8 5.2S4.2 12 4.2 12z" />
            <circle cx="12" cy="12" r="2.1" />
            <path d="M5 19 19 5" />
          {:else}
            <path d="M7 7l10 10" />
            <path d="M17 7L7 17" />
          {/if}
        </svg>
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
    gap: var(--space-2);
    height: 2.75rem;
    padding: 0 0.35rem 0 1rem;
    background: var(--canvas);
    border-bottom: 1px solid var(--line);
    color: var(--fg);
    user-select: none;
  }

  /* The wordmark is the one place the product names itself. The dot is the mark: one accent, small,
     and it is not a button and not a status — it only says "this is mirk". A pseudo-element keeps the
     markup unchanged, so the accessible name is still just the product's name. */
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.95rem;
    font-weight: var(--weight-semibold);
    letter-spacing: -0.01em;
  }

  .brand::before {
    content: "";
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 50%;
    background: var(--brand);
    box-shadow: 0 0 10px rgba(255, 99, 99, 0.55);
  }

  .controls {
    display: flex;
    gap: 0.1rem;
  }

  button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.75rem;
    height: 1.75rem;
    padding: 0;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: 0.8rem;
    line-height: 1;
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  button:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--fg);
  }

  button.close:hover:not(:disabled) {
    background: var(--danger);
    color: #ffffff;
  }

  button:disabled {
    opacity: 0.35;
    cursor: default;
  }
</style>
