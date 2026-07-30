<script lang="ts">
  // One warning Banner per risk (DESIGN §10 — severity comes from the tone token
  // families, never ad-hoc colour). The `customizations` risk can render two
  // lines from one entry: user-added mods and manually-dropped jars carry
  // different confidence and read better apart.
  //
  // `risk.kind` is a safe {#each} key — assessSwitchRisks emits each kind at
  // most once.
  import type { SwitchRisk } from './switch-risks';
  import { t } from '$lib/i18n';
  import Banner from '$lib/ui/Banner.svelte';

  let { risks }: { risks: SwitchRisk[] } = $props();
</script>

{#if risks.length > 0}
  <!-- One `role="alert"` for the whole group, not one per Banner: up to five
       risks mount at once, and five simultaneous alert regions announce as a
       jumble. The group is the thing the user needs read out. -->
  <div class="flex flex-col gap-2" role="alert" data-testid="switch-risk-list">
    {#each risks as risk (risk.kind)}
      <Banner tone="warning" icon="warning" dataTestid={`switch-risk-${risk.kind}`}>
        {#if risk.kind === 'mc-change'}
          {$t('modpacks.switch.riskMcChange', { from: risk.from, to: risk.to })}
        {:else if risk.kind === 'downgrade'}
          {$t('modpacks.switch.riskDowngrade')}
        {:else if risk.kind === 'loader-change'}
          {$t('modpacks.switch.riskLoaderChange', { from: risk.from, to: risk.to })}
        {:else if risk.kind === 'customizations'}
          <div class="flex flex-col gap-1">
            {#if risk.userAdded > 0}
              <span>{$t('modpacks.switch.riskCustomizationsUser', { count: risk.userAdded })}</span>
            {/if}
            {#if risk.manual > 0}
              <span>{$t('modpacks.switch.riskCustomizationsManual', { count: risk.manual })}</span>
            {/if}
          </div>
        {:else if risk.kind === 'bundled-overrides'}
          {$t('modpacks.switch.riskBundledOverrides')}
        {/if}
      </Banner>
    {/each}
  </div>
{/if}
