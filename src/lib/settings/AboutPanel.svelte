<script lang="ts">
  // About section of Settings — required for Minecraft Usage Guidelines
  // compliance (verbatim disclaimer + identity of who ships this).
  // The disclaimer text lives in `./disclaimer.ts` so the test file and
  // the panel never disagree on the exact string. No page title: the tab
  // says "About"; three blocks with the shared h3 recipe (DESIGN §3).
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import pkg from '../../../package.json' with { type: 'json' };
  import { LICENSE_URL, PRIVACY_POLICY_URL, REPO_URL } from './disclaimer';
  import { tooltip } from '$lib/ui/tooltip';
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';
  import { type BuildInfo, commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import SettingsField from './SettingsField.svelte';
  import { buildLine, versionInfoBlock } from './version-info';

  const version = pkg.version;

  // Loaded on mount, not on the click: the copy should not wait on an IPC
  // round-trip, and the line under the version needs it anyway. A failed read
  // is null — the copy then says "unknown" rather than leaving a field blank.
  let buildInfo = $state<BuildInfo | null>(null);
  let buildInfoLoad: Promise<BuildInfo | null> = Promise.resolve(null);

  onMount(() => {
    buildInfoLoad = (async () => {
      try {
        return await commands.appBuildInfo();
      } catch {
        return null;
      }
    })();
    void buildInfoLoad.then((info) => {
      buildInfo = info;
    });
  });

  async function copyVersionInfo() {
    const tr = get(t);
    const text = versionInfoBlock(await buildInfoLoad, version);
    try {
      const r = await commands.clipboardWriteText(text);
      if (r.status === 'ok') pushSuccess(tr('settings.about.versionInfoCopied'));
      else pushWarning(tr('settings.about.versionInfoCopyFailed'), [formatError(r.error)]);
    } catch {
      // The command itself could not be reached; nothing was copied.
      pushWarning(tr('settings.about.versionInfoCopyFailed'));
    }
  }
</script>

<section class="flex flex-col gap-6 text-sm selectable">
  <div class="flex flex-col gap-2">
    <h3 class="font-medium text-sm text-primary">{$t('settings.about.identityTitle')}</h3>
    <p class="font-medium text-primary">Lucerna v{version}</p>
    {#if buildInfo}
      <p class="text-xs text-muted" data-testid="about-build">{buildLine(buildInfo, $t)}</p>
    {/if}
    <p class="text-secondary">{$t('settings.about.disclaimer')}</p>
    <SettingsField anchor="about.repo">
      <p>
        <button
          type="button"
          class="btn-link inline-flex items-center gap-1"
          use:tooltip={REPO_URL}
          onclick={() => void openExternalHttps(REPO_URL)}
        >
          {$t('settings.about.viewOnGitHub')}
          <Icon name="externalLink" size={14} />
        </button>
      </p>
    </SettingsField>
    <SettingsField anchor="about.versionInfo">
      <div>
        <button
          type="button"
          class="btn-secondary btn-sm inline-flex items-center gap-1.5"
          onclick={() => void copyVersionInfo()}
        >
          <Icon name="copy" size={14} />
          {$t('settings.about.copyVersionInfo')}
        </button>
      </div>
    </SettingsField>
  </div>

  <div class="flex flex-col gap-2">
    <h3 class="font-medium text-sm text-primary">{$t('settings.about.dataTitle')}</h3>
    <SettingsField anchor="about.privacyPolicy">
      <div class="flex flex-col items-start gap-1">
        <p class="text-xs text-muted">{$t('settings.about.noTelemetry')}</p>
        <button
          type="button"
          class="btn-link inline-flex items-center gap-1 text-xs"
          use:tooltip={PRIVACY_POLICY_URL}
          onclick={() => void openExternalHttps(PRIVACY_POLICY_URL)}
        >
          {$t('settings.about.privacyPolicy')}
          <Icon name="externalLink" size={12} />
        </button>
      </div>
    </SettingsField>
    <p class="text-xs text-muted">{$t('settings.about.keyring')}</p>
  </div>

  <div class="flex flex-col gap-2">
    <h3 class="font-medium text-sm text-primary">{$t('settings.about.legalTitle')}</h3>
    <SettingsField anchor="about.license">
      <div class="flex flex-col items-start gap-1">
        <p class="text-xs text-muted">{$t('settings.about.license')}</p>
        <button
          type="button"
          class="btn-link inline-flex items-center gap-1 text-xs"
          use:tooltip={LICENSE_URL}
          onclick={() => void openExternalHttps(LICENSE_URL)}
        >
          {$t('settings.about.readLicense')}
          <Icon name="externalLink" size={12} />
        </button>
      </div>
    </SettingsField>
    <p class="text-xs text-muted">{$t('settings.about.mojangFiles')}</p>
    <p class="text-xs text-muted">{$t('settings.about.trademark')}</p>
  </div>
</section>
