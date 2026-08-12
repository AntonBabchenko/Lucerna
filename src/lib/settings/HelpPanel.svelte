<script lang="ts">
  // Settings → Help. Tip detail level + replay the onboarding tour.
  import { tick } from 'svelte';
  import { type ExplanationLevel } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import { explanationState, setExplanationLevel } from '$lib/onboarding/explanation-level.svelte';
  import { replayTour } from '$lib/onboarding/state.svelte';
  import { settingsOpen } from './state.svelte';
  import SettingsField from './SettingsField.svelte';

  const tipsOptions = $derived<{ value: ExplanationLevel; label: string }[]>([
    { value: 'basic', label: $t('settings.general.tips.basic') },
    { value: 'advanced', label: $t('settings.general.tips.advanced') },
  ]);

  async function onReplay() {
    // Close the settings modal FIRST, then start the tour once it has unmounted
    // — otherwise the main tour spotlights anchors that are still hidden behind
    // the open modal, so the tour appears to do nothing.
    settingsOpen.value = null;
    await tick();
    replayTour();
  }
</script>

<section class="flex flex-col gap-6">
  <SettingsField anchor="help.tipsLevel">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.tips.title')}</h3>
      <div class="flex flex-col gap-1">
        <span class="text-sm text-primary">{$t('settings.general.tips.levelLabel')}</span>
        <Select
          class="text-sm"
          dataTestid="tip-level-select"
          ariaLabel={$t('settings.general.tips.levelLabel')}
          value={explanationState.level}
          options={tipsOptions}
          onChange={(v) => void setExplanationLevel(v as ExplanationLevel)}
        />
        <span class="text-xs text-muted">{$t('settings.general.tips.levelDescription')}</span>
      </div>
    </div>
  </SettingsField>

  <SettingsField anchor="help.replayTours">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.onboarding.title')}</h3>
      <div class="flex items-center gap-3">
        <button type="button" class="btn-secondary btn-sm" onclick={onReplay}>
          {$t('settings.general.onboarding.replayBtn')}
        </button>
        <p class="text-xs text-muted">{$t('settings.general.onboarding.replayDescription')}</p>
      </div>
    </div>
  </SettingsField>
</section>
