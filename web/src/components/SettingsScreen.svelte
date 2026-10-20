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
  }: {
    t: Translate;
    /** Opens one sub-screen. App wires it to the hash, so the choice is addressable (R43/S2). */
    onOpenSection: (section: SettingsSection) => void;
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
          <span class="name">{settingsSectionTitle(section, t)}</span>
          <span class="summary">{settingsSectionSummary(section, t)}</span>
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
    font-size: 1.35rem;
  }

  .intro {
    margin: 0;
    color: #5a6472;
    font-size: 0.85rem;
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
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    width: 100%;
    box-sizing: border-box;
    padding: 0.7rem 0.9rem;
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
    font: inherit;
    text-align: left;
    color: #1c2024;
    cursor: pointer;
  }

  .section:hover {
    border-color: #24506b;
    background: #f7f9fb;
  }

  .name {
    font-size: 0.95rem;
    font-weight: 600;
    color: #17364a;
  }

  .summary {
    font-size: 0.8rem;
    color: #5a6472;
  }
</style>
