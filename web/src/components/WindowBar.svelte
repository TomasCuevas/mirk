<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import { windowControls, type WindowControl } from "../lib/window";

  let {
    t,
    inTauri,
    maximized,
    onAction,
    onDrag,
    onToggle,
  }: {
    t: Translate;
    inTauri: boolean;
    /** Whether the full-screen layout is in force, which flips the maximise control to restore. */
    maximized: boolean;
    onAction: (control: WindowControl) => void;
    onDrag: () => void;
    /**
     * The double click on the bar. The parent decides what a toggle means — the stored layout, exactly
     * as the maximise control does — so the bar only reports the gesture.
     */
    onToggle?: () => void;
  } = $props();

  // The bar is the window's drag handle: pressing it (but not one of its buttons) starts the move
  // gesture through an application command. The second press of a double click asks the parent to
  // toggle the window instead, and `detail` is what says so — the first press has already handed the
  // window to the compositor's drag loop, and that loop is what would eat a `dblclick`.
  function handleMouseDown(event: MouseEvent): void {
    if (!inTauri || event.button !== 0) {
      return;
    }
    // R5: `Element`, not `HTMLElement`. The target of a press on a control is usually the <svg> or a
    // <path> inside it, and asking an HTMLElement swallowed the press into `start_dragging` — the
    // gesture took the pointer, the click never completed, and the control only answered on the
    // presses that happened to land on its padding.
    if (event.target instanceof Element && event.target.closest("button") !== null) {
      return;
    }
    if (event.detail >= 2) {
      onToggle?.();
      return;
    }
    onDrag();
  }
</script>

<!--
  The custom top bar (A2). The application is undecorated on purpose, so this is the only title bar
  there is: pressing it moves the window, a double click toggles the two window states, and it draws
  exactly the four controls the user asked for — minimise, maximise/restore, hide and close (S4 of
  `odd/tasks/desktop-chrome.md`). Maximise is not a second size authority: it chooses the same stored
  layout Ajustes → Ventana does, which keeps the button, the double click and the selected option in
  agreement, and the bar stays visible in full screen so closing and hiding remain reachable there.
-->
<!-- The bar is a drag handle, not a control; its own buttons carry the interactive semantics. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<header onmousedown={handleMouseDown}>
  <span class="brand">{t("app.name")}</span>

  <div class="controls" role="group" aria-label={t("window.controls")}>
    {#each windowControls as control (control.action)}
      {@const restore = control.action === "maximize" && maximized}
      {@const labelKey = restore ? "window.restore" : control.labelKey}
      {@const icon = restore ? "restore" : control.icon}
      <button
        type="button"
        class:close={control.action === "close"}
        class:pressed={restore}
        title={t(labelKey)}
        aria-label={t(labelKey)}
        aria-pressed={control.action === "maximize" ? maximized : undefined}
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
          {#if icon === "minimize"}
            <path d="M6 12h12" />
          {:else if icon === "maximize"}
            <rect x="6" y="6" width="12" height="12" rx="1.6" />
          {:else if icon === "restore"}
            <!-- The state the window is in when the control is pressed: stacked panes, the shape the
                 platform uses for "restore". -->
            <rect x="5.5" y="8" width="9.5" height="9.5" rx="1.4" />
            <path d="M9 8V6.5A1.5 1.5 0 0 1 10.5 5h7A1.5 1.5 0 0 1 19 6.5v7a1.5 1.5 0 0 1-1.5 1.5H16" />
          {:else if icon === "hide"}
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
  /* The bar's words are rem-sized, so its drawings are too: at the full-screen root the bar is taller and
     a 16 px glyph would read as an undersized control. */
  button svg {
    width: 1rem;
    height: 1rem;
  }

  header {
    flex: none;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.015)), var(--glass);
    backdrop-filter: blur(var(--glass-blur)) saturate(1.08);
    -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(1.08);
    border-bottom: 1px solid var(--glass-border);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    height: 2.75rem;
    padding: 0 0.35rem 0 1rem;
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
    box-shadow: 0 0 14px var(--brand-glow), 0 0 3px var(--brand);
    /* L4: the one moving glint in the interface — slow enough to read as a reflection rather than a
       notification, and still for anyone who asked for less motion. */
    animation: brand-glint 7s ease-in-out infinite;
  }

  @keyframes brand-glint {
    0%,
    100% {
      box-shadow: 0 0 12px var(--brand-glow), 0 0 3px var(--brand);
    }
    50% {
      box-shadow: 0 0 22px var(--brand-glow), 0 0 5px var(--brand);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .brand::before {
      animation: none;
    }
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

  /* The maximised state is a state, not only a label: the control stays lifted while the window is
     full screen, the same way a pressed toggle does. */
  button.pressed:not(:disabled) {
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
