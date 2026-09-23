<script lang="ts">
  // Settings → Help. Tip detail level (a SegmentedControl with ONE hint that
  // follows the selection) + replay the onboarding tour + the support block
  // (report a bug, the launcher's own log folder, where security reports go).
  import { tick } from 'svelte';
  import { commands, type ExplanationLevel } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { Icon } from '$lib/ui/icons';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import { tooltip } from '$lib/ui/tooltip';
  import { BUG_REPORT_URL, SECURITY_POLICY_URL } from './disclaimer';
  import { t } from '$lib/i18n';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import { explanationState, setExplanationLevel } from '$lib/onboarding/explanation-level.svelte';
  import { saveFailure } from '$lib/settings/app-settings.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { replayTour } from '$lib/onboarding/state.svelte';
  import { closeSettings } from './state.svelte';
  import SettingsField from './SettingsField.svelte';

  const tipsOptions = $derived([
    { value: 'basic', label: $t('settings.general.tips.basic') },
    { value: 'advanced', label: $t('settings.general.tips.advanced') },
  ]);
  // The text after the em-dash of the old option labels, one line, for the
  // level that is selected.
  const tipsHint = $derived(
    explanationState.level === 'advanced'
      ? $t('settings.general.tips.hintAdvanced')
      : $t('settings.general.tips.hintBasic'),
  );

  async function onReplay() {
    // Close the settings modal FIRST, then start the tour once it has unmounted
    // — otherwise the main tour spotlights anchors that are still hidden behind
    // the open modal, so the tour appears to do nothing.
    closeSettings();
    await tick();
    replayTour();
  }

  // The folder open can fail (no opener, a path that cannot be created); say
  // why under the button instead of doing nothing.
  let logFolderError = $state<string | null>(null);
  async function openLogFolder() {
    logFolderError = null;
    const r = await commands.openLauncherLogFolder();
    if (r.status === 'error') {
      logFolderError = $t('settings.help.support.logFolderFailed', { error: formatError(r.error) });
    }
  }
</script>

<section class="flex flex-col gap-6">
  <SettingsField anchor="help.tipsLevel">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.tips.title')}</h3>
      <p class="text-xs text-muted">{$t('settings.general.tips.levelDescription')}</p>
      <div class="flex flex-col gap-1">
        <span class="text-sm text-primary">{$t('settings.general.tips.levelLabel')}</span>
        <SegmentedControl
          variant="boxed"
          ariaLabel={$t('settings.general.tips.levelLabel')}
          dataTestid="tip-level-select"
          describedby="help-tips-hint"
          options={tipsOptions}
          value={explanationState.level}
          onChange={(v) => void setExplanationLevel(v as ExplanationLevel)}
        />
        <div data-testid="save-failure-explanation_level">
          <StatusMessage message={saveFailure('explanation_level')} tone="danger" />
        </div>
        <p id="help-tips-hint" class="text-xs text-muted">{tipsHint}</p>
      </div>
    </div>
  </SettingsField>

  <SettingsField anchor="help.replayTours">
    <div class="flex flex-col gap-3">
      <h3 class="font-medium text-sm text-primary">{$t('settings.general.onboarding.title')}</h3>
      <!-- The description sits UNDER the button (like the tips row), and the
           button never wraps (HELP-09). -->
      <div class="flex flex-col items-start gap-1">
        <button
          type="button"
          class="btn-secondary btn-sm shrink-0"
          aria-describedby="help-replay-description"
          onclick={onReplay}
        >
          {$t('settings.general.onboarding.replayBtn')}
        </button>
        <p id="help-replay-description" class="text-xs text-muted">
          {$t('settings.general.onboarding.replayDescription')}
        </p>
      </div>
    </div>
  </SettingsField>

  <div class="flex flex-col gap-3">
    <h3 class="font-medium text-sm text-primary">{$t('settings.help.support.title')}</h3>
    <SettingsField anchor="help.support">
      <div class="flex flex-col items-start gap-1">
        <button
          type="button"
          class="btn-link inline-flex items-center gap-1"
          aria-describedby="help-report-bug-description"
          use:tooltip={BUG_REPORT_URL}
          onclick={() => void openExternalHttps(BUG_REPORT_URL)}
        >
          {$t('settings.help.support.reportBug')}
          <Icon name="externalLink" size={14} />
        </button>
        <p id="help-report-bug-description" class="text-xs text-muted">
          {$t('settings.help.support.reportBugHint')}
        </p>
      </div>
    </SettingsField>
    <SettingsField anchor="help.logFolder">
      <div class="flex flex-col items-start gap-1">
        <button
          type="button"
          class="btn-secondary btn-sm inline-flex items-center gap-1.5 shrink-0"
          onclick={() => void openLogFolder()}
        >
          <Icon name="folderOpen" size={14} />
          {$t('settings.help.support.openLogFolder')}
        </button>
        <StatusMessage message={logFolderError} tone="danger" />
      </div>
    </SettingsField>
    <div class="flex flex-col items-start gap-1">
      <p class="text-xs text-muted">{$t('settings.help.support.security')}</p>
      <button
        type="button"
        class="btn-link inline-flex items-center gap-1 text-xs"
        use:tooltip={SECURITY_POLICY_URL}
        onclick={() => void openExternalHttps(SECURITY_POLICY_URL)}
      >
        {$t('settings.help.support.securityPolicy')}
        <Icon name="externalLink" size={12} />
      </button>
    </div>
  </div>
</section>
