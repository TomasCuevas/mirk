<script lang="ts">
  import type { Locale, Translate } from "../lib/i18n";
  import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";
  import { updateAction, updateLine, type UpdateState } from "../lib/update";

  let {
    locale,
    t,
    onLocale,
    onBack,
    update,
    onCheck,
    onInstall,
  }: {
    locale: Locale;
    t: Translate;
    onLocale: (locale: Locale) => void;
    /** Returns to the Ajustes index (R43/S2), through the same hash navigation as everything else. */
    onBack: () => void;
    /**
     * S7 of `odd/tasks/ci-and-release.md`: the update check's state.
     *
     * It is a PROP and not this screen's own state because the shell owns the check: it runs when the
     * window opens, before any screen exists, and the mark on the Ajustes index reads the same value.
     * A screen that checked on its own would either check a second time on every visit or know nothing
     * until the reader walked in.
     */
    update: UpdateState;
    onCheck: () => void;
    onInstall: () => void;
  } = $props();

  // What the one control does depends on what the check said, so the label is derived from the state
  // rather than from a second flag this screen would have to keep in step.
  const action = $derived(updateAction(update, t));

  // R15: two options are not a dropdown. A select hides one of them behind a click and draws the
  // engine's own chrome; a segmented control shows both and states the current one by position.
  const languages = [
    { code: "en", key: "language.en" },
    { code: "es", key: "language.es" },
  ] as const;
</script>

<!--
  R43/S3 (L71). The Aplicación sub-screen: the interface language.

  It is one selector and one line of explanation, and the explanation is the part the old screen never
  had: the choice is not private to this window — the desktop process builds the tray's words from the
  same stored value (R1) — so the sentence says that, rather than leaving the reader to find out.
-->
<section class="screen">
  <!-- R43/S2: a way back, named after the screen it returns to. -->
  <!-- R15: the arrow is the affordance; the label names where it goes. -->
  <button
    type="button"
    class="back"
    aria-label={t("settings.backLabel")}
    onclick={onBack}
  >
    <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor"
        stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M12.5 8h-9" />
        <path d="M7.5 4 3.5 8l4 4" />
      </svg>
    {t("settings.back")}
  </button>

  <h1>{settingsSectionTitle("app", t)}</h1>
  <!-- R43/S4: one line saying what this screen is for. -->
  <p class="purpose">{settingsSectionPurpose("app", t)}</p>

  <div class="panel">
    <h2>{t("settings.language.heading")}</h2>
    <div class="segmented" role="group" aria-label={t("language.label")}>
      {#each languages as language (language.code)}
        <button
          type="button"
          class:current={locale === language.code}
          aria-pressed={locale === language.code}
          onclick={() => onLocale(language.code as Locale)}
        >
          {t(language.key)}
        </button>
      {/each}
    </div>
    <p class="detail">{t("settings.language.hint")}</p>
  </div>

  <!--
    S7. The update check, discreet by the reader's own choice.

    The shell asks GitHub when the window opens and this panel is the only place that says what came
    back: a line of state, the release's own notes when it has any, and one control that names what it
    will do. Nothing is downloaded until that control is pressed — an update channel that installs on its
    own turns a background request into a change of the program a reader is using.
  -->
  <div class="panel">
    <h2>{t("settings.update.heading")}</h2>
    <p class="detail state">{updateLine(update, t)}</p>
    {#if update.kind === "available" && update.offer.notes !== null}
      <!-- The release body as GitHub returned it: the author's words, not a summary of them. -->
      <p class="notes">{update.offer.notes}</p>
    {/if}
    {#if action !== null}
      <button type="button" class="action" onclick={action.kind === "install" ? onInstall : onCheck}>
        {action.label}
      </button>
    {/if}
    <p class="detail">{t("settings.update.hint")}</p>
  </div>
</section>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  h1 {
    margin: 0;
  }

  .back {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    align-self: flex-start;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: var(--surface-1);
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .back:hover {
    background: var(--surface-2);
    color: var(--fg);
  }

  .purpose {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--fg-muted);
  }

  /* The block's name, as an eyebrow: small caps is what tells a reader this line names the block below
     it rather than starting a paragraph. */
  h2 {
    margin: 0 0 0.75rem;
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
    text-transform: uppercase;
    letter-spacing: var(--tracking-wide);
    color: var(--fg);
  }

  .panel {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.018));
    border: 1px solid var(--glass-border);
    border-radius: var(--r-lg);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
    padding: 0.8rem 0.9rem;
  }

  /* The language is a choice between exactly two things, so both are on screen: the current one is
     the raised half of the pill, and choosing the other is one click rather than two. */
  .segmented {
    display: flex;
    gap: 0.2rem;
    padding: 0.2rem;
    background: var(--surface-1);
    border: 1px solid var(--line);
    border-radius: var(--r-md);
  }

  .segmented button {
    flex: 1 1 0;
    height: 2.25rem;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--fast) var(--ease), color var(--fast) var(--ease);
  }

  .segmented button:hover:not(.current) {
    color: var(--fg);
  }

  .segmented button.current {
    background: linear-gradient(180deg, rgba(196, 122, 168, 0.26), rgba(196, 122, 168, 0.12));
    color: var(--fg);
    font-weight: var(--weight-semibold);
  }

  .detail {
    margin: 0.5rem 0 0;
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  /* The state line is the answer, so it is the one line in the panel that is not muted. */
  .state {
    color: var(--fg);
  }

  /* Release notes arrive with the author's own line breaks, and they are shown as written. */
  .notes {
    margin: 0.4rem 0 0;
    font-size: var(--text-xs);
    color: var(--fg-subtle);
    white-space: pre-wrap;
  }

  /* The one control, in the same tint the current half of the segmented pill uses: this screen has one
     accent for "the thing that is chosen" and one for "the thing to do", and they are the same family. */
  .action {
    display: inline-flex;
    align-items: center;
    align-self: flex-start;
    margin-top: 0.6rem;
    padding: 0.45rem 0.8rem;
    border: 1px solid var(--glass-border);
    border-radius: var(--r-sm);
    background: linear-gradient(180deg, rgba(196, 122, 168, 0.26), rgba(196, 122, 168, 0.12));
    color: var(--fg);
    font: inherit;
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
    cursor: pointer;
    transition: background var(--fast) var(--ease), border-color var(--fast) var(--ease);
  }

  .action:hover {
    background: linear-gradient(180deg, rgba(196, 122, 168, 0.36), rgba(196, 122, 168, 0.18));
    border-color: var(--line-strong);
  }


</style>
