<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import {
    settingsSectionSummary,
    settingsSectionTitle,
    settingsSections,
    type SettingsSection,
  } from "../lib/screens";

  let {
    t,
    onOpenSection,
    updateAvailable,
  }: {
    t: Translate;
    /** Opens one sub-screen. App wires it to the hash, so the choice is addressable (R43/S2). */
    onOpenSection: (section: SettingsSection) => void;
    /**
     * S7 of `odd/tasks/ci-and-release.md`: whether a newer version is waiting in Aplicación.
     *
     * It is a Boolean rather than the check's state because the index has exactly one thing to say about
     * the update — that there is one — and the sentence behind it belongs to the screen that can act on
     * it. The mark sits on the Aplicación row, which is where the reader has to go.
     */
    updateAvailable: boolean;
  } = $props();
</script>

<!--
  R43/S1 (L71). Ajustes is an INDEX, and this is what the round was opened about.

  It used to be one long page with five flat blocks of equal weight — Language, Progress providers,
  Watched folders, Suggestions, Scan — and the user's words were that it is *"nada intuitiva"*. Nothing
  was missing; the page simply never said which of five unrelated things a person was looking at. So the
  contents moved into three sub-screens, each named for one kind of setting, and this page now shows
  three rows: a name and ONE plain sentence saying what lives inside, and the click that opens it.

  Every row is the whole row rather than a link at its end, because a person chooses a section, not a
  word: the name and the sentence are one target, so the thing a reader aims at is the thing they are
  reading. Nothing here is a setting — the settings themselves are untouched, one click away (R43/S5).
-->
<section class="screen">
  <h1>{t("settings.heading")}</h1>
  <!-- One line, and it is not decoration: it is what tells a reader that Ajustes is grouped at all. -->
  <p class="intro">{t("settings.intro")}</p>

  <ul class="sections">
    {#each settingsSections as section (section)}
      <li>
        <button type="button" class="section" onclick={() => onOpenSection(section)}>
          <!-- S3 of `odd/tasks/desktop-chrome.md`: one drawing per section, in the navigation's hand.
               Decorative: the row's own name is what a screen reader reads. -->
          <svg
            viewBox="0 0 24 24"
            width="20"
            height="20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.4"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            focusable="false"
          >
            {#if section === "library"}
              <path d="M4.6 5.2h3.6v13.6H4.6z" />
              <path d="M9.8 5.2h3.6v13.6H9.8z" />
              <path d="m15.1 6.3 3.5-.7 2.2 12.9-3.5.6z" />
            {:else if section === "reading"}
              <path d="M12 6.6C10 5.2 7.6 4.8 4.6 5.2v12.6c3-.4 5.4 0 7.4 1.4 2-1.4 4.4-1.8 7.4-1.4V5.2c-3-.4-5.4 0-7.4 1.4z" />
              <path d="M12 6.6v12.6" />
            {:else if section === "app"}
              <circle cx="12" cy="12" r="7.4" />
              <path d="M4.6 12h14.8" />
              <path d="M12 4.6c2 2.2 3 4.7 3 7.4s-1 5.2-3 7.4c-2-2.2-3-4.7-3-7.4s1-5.2 3-7.4z" />
            {:else}
              <rect x="4.4" y="5.4" width="15.2" height="13.2" rx="1.8" />
              <path d="M4.4 9.6h15.2" />
              <path d="M7 7.5h.01M9.4 7.5h.01" />
            {/if}
          </svg>
          <span class="text">
            <span class="name">
              {settingsSectionTitle(section, t)}
              {#if section === "app" && updateAvailable}
                <!-- S7: the discreet notice the reader chose — a mark, not a modal. It is announced
                     rather than decorative, because a dot that only exists visually says nothing to a
                     screen reader, and "there is a newer version" is the whole message. -->
                <span class="waiting" role="img" aria-label={t("settings.update.dot")}></span>
              {/if}
            </span>
            <span class="summary">{settingsSectionSummary(section, t)}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>
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

  .intro {
    margin: 0;
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  /* Each row is a card, the same shape the panels inside the sub-screens use: the index and its
     contents read as one family instead of as a menu bolted onto a form. */
  .section {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 0.65rem;
    width: 100%;
    box-sizing: border-box;
    padding: 0.8rem 0.9rem;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.018));
    border: 1px solid var(--glass-border);
    border-radius: var(--r-lg);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
    font: inherit;
    text-align: left;
    color: var(--fg);
    cursor: pointer;
    transition: background var(--fast) var(--ease), border-color var(--fast) var(--ease);
  }

  /* The hover is a light passing over the pane, not a fill swap: the same gradient, brighter, with the
     top edge catching it and a breath of the wallpaper's pink underneath. */
  .section:hover {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.075), rgba(255, 255, 255, 0.03));
    border-color: var(--line-strong);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08), 0 0 24px rgba(230, 163, 201, 0.07);
  }

  .section svg {
    flex: none;
    margin-top: 0.1rem;
    color: var(--fg-subtle);
  }

  .section:hover svg {
    color: var(--fg-muted);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }

  .name {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg);
  }

  /* The mark: the same accent the shell uses for a chosen thing, small enough that a reader who does not
     care about updates can ignore it and large enough that one who does will not miss it. */
  .waiting {
    width: 0.4rem;
    height: 0.4rem;
    flex: none;
    border-radius: 50%;
    background: rgb(196, 122, 168);
    box-shadow: 0 0 0 3px rgba(196, 122, 168, 0.18);
  }

  .summary {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }
</style>
