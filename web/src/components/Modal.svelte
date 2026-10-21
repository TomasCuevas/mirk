<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onClose,
    children,
  }: {
    title: string;
    onClose: () => void;
    children: Snippet;
  } = $props();

  let dialog = $state<HTMLDivElement | null>(null);

  // Focus moves into the dialog as it opens, so Escape reaches it and a keyboard reader carries on
  // inside the modal instead of tabbing through the screen behind it. A full focus trap would need a
  // dependency this project does not have; this is the part that makes the modal usable without one.
  $effect(() => {
    dialog?.focus();
  });

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      onClose();
    }
  }

  // The backdrop answers a click as well, because a modal that cannot be dismissed without finding the
  // right button is a trap. Both modals in this project treat "dismissed" as the safe answer: cancelling
  // a removal and closing a refusal both leave the store exactly as it was. Its keyboard equivalent is
  // Escape, handled on the window above — the two are the same gesture, which is why the click needs no
  // handler of its own on a focusable control.
</script>

<svelte:window onkeydown={onKeydown} />

<!--
  A modal of our own rather than the native message box the dialog plugin can open (R26). Two reasons:
  the words stay in the catalogue with every other word of the interface, and the removal's confirmation
  has to state a count, which a plain message box cannot be shaped to do.
-->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" role="presentation" onclick={onClose}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    bind:this={dialog}
    onclick={(event) => event.stopPropagation()}
  >
    <h3>{title}</h3>
    {@render children()}
  </div>
</div>

<style>
  /* The backdrop is the canvas pushed back, not a grey veil: 72 % black over an already black screen
     darkens what is behind the dialog without tinting it, and the blur is what separates the dialog
     from the cards underneath rather than the shadow alone. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1rem;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(4px);
  }

  .dialog {
    width: 100%;
    max-width: 22rem;
    max-height: 100%;
    overflow-y: auto;
    /* A dialog scrolls vertically and never sideways. CSS makes the other axis `auto` as soon as one
       is not `visible`, so a single element that pokes out of the box — a tooltip, a long unbroken
       word — turns the whole editor into a horizontally scrolling surface. `hidden` costs nothing and
       removes the failure mode; the popover that caused it now grows inwards as well. */
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 1rem;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-modal);
  }

  /* The focus is moved here by the script, not by the reader's tab: a ring around the whole card would
     mark the container rather than anything the person can act on. The buttons inside keep theirs. */
  .dialog:focus {
    outline: none;
  }

  h3 {
    margin: 0;
    font-size: var(--text-base);
    font-weight: var(--weight-semibold);
    color: var(--fg);
  }
</style>
