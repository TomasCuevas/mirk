<script lang="ts">
  import type { Translate } from "../lib/i18n";
  import { homeSections } from "../lib/screens";

  let { t }: { t: Translate } = $props();
</script>

<!--
  Inicio is an honest placeholder (L19): it names the three summaries it will show and shows no
  number, because the queries behind those numbers do not exist yet.
-->
<section class="screen">
  <h1>{t("home.heading")}</h1>
  <p class="state">{t("home.placeholder")}</p>

  <ul class="promises">
    {#each homeSections(t) as section (section.title)}
      <li>
        <span class="title">{section.title}</span>
        <span class="detail">{section.detail}</span>
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

  .state {
    margin: 0;
    color: #5a6472;
    max-width: 42rem;
  }

  .promises {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.75rem;
  }

  .promises li {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.7rem 0.8rem;
    background: #ffffff;
    border: 1px solid #e2e6eb;
    border-radius: 0.75rem;
  }

  .title {
    font-weight: 600;
  }

  .detail {
    color: #5a6472;
    font-size: 0.9rem;
  }
</style>
