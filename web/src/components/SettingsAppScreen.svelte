<script lang="ts">
  import type { Locale, Translate } from "../lib/i18n";
  import { settingsSectionPurpose, settingsSectionTitle } from "../lib/screens";

  let {
    locale,
    t,
    onLocale,
    onBack,
  }: {
    locale: Locale;
    t: Translate;
    onLocale: (locale: Locale) => void;
    /** Returns to the Ajustes index (R43/S2), through the same hash navigation as everything else. */
    onBack: () => void;
  } = $props();
</script>

<!--
  R43/S3 (L71). The Aplicación sub-screen: the interface language.

  It is one selector and one line of explanation, and the explanation is the part the old screen never
  had: the choice is not private to this window — the desktop process builds the tray's words from the
  same stored value (R1) — so the sentence says that, rather than leaving the reader to find out.
-->
<section class="screen">
  <!-- R43/S2: a way back, named after the screen it returns to. -->
  <button type="button" class="back" onclick={onBack}>{t("settings.back")}</button>

  <h1>{settingsSectionTitle("app", t)}</h1>
  <!-- R43/S4: one line saying what this screen is for. -->
  <p class="purpose">{settingsSectionPurpose("app", t)}</p>

  <div class="panel">
    <h2>{t("settings.language.heading")}</h2>
    <label class="inline">
      <span>{t("language.label")}</span>
      <select
        value={locale}
        onchange={(event) => onLocale(event.currentTarget.value as Locale)}
      >
        <option value="en">{t("language.en")}</option>
        <option value="es">{t("language.es")}</option>
      </select>
    </label>
    <p class="detail">{t("settings.language.hint")}</p>
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
    font-size: 1.35rem;
  }

  .back {
    align-self: flex-start;
    padding: 0.35rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    color: #24506b;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .back:hover {
    background: #f0f2f5;
  }

  .purpose {
    margin: 0;
    font-size: 0.85rem;
    color: #5a6472;
  }

  h2 {
    margin: 0 0 0.75rem;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #5a6472;
  }

  .panel {
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
    padding: 0.7rem 0.9rem;
  }

  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.85rem;
    color: #5a6472;
  }

  .detail {
    margin: 0.5rem 0 0;
    font-size: 0.8rem;
    color: #5a6472;
  }

  select {
    padding: 0.45rem 0.6rem;
    border: 1px solid #ccd2da;
    border-radius: 0.4rem;
    background: #ffffff;
    font: inherit;
    font-size: 0.9rem;
    color: #1c2024;
    box-sizing: border-box;
    max-width: 100%;
  }
</style>
