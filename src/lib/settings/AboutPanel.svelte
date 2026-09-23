<script lang="ts">
  // About section of Settings — required for Minecraft Usage Guidelines
  // compliance (verbatim disclaimer + identity of who ships this).
  // The disclaimer text lives in `./disclaimer.ts` so the test file and
  // the panel never disagree on the exact string. No page title: the tab
  // says "About"; three blocks with the shared h3 recipe (DESIGN §3).
  import pkg from '../../../package.json' with { type: 'json' };
  import { REPO_URL } from './disclaimer';
  import { tooltip } from '$lib/ui/tooltip';
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import SettingsField from './SettingsField.svelte';

  const version = pkg.version;

  function openRepo() {
    void openExternalHttps(REPO_URL);
  }
</script>

<section class="flex flex-col gap-6 text-sm selectable">
  <div class="flex flex-col gap-2">
    <h3 class="font-medium text-sm text-primary">{$t('settings.about.identityTitle')}</h3>
    <p class="font-medium text-primary">Lucerna v{version}</p>
    <p class="text-secondary">{$t('settings.about.disclaimer')}</p>
    <SettingsField anchor="about.repo">
      <p>
        <button
          type="button"
          class="btn-link inline-flex items-center gap-1"
          use:tooltip={REPO_URL}
          onclick={openRepo}
        >
          {$t('settings.about.viewOnGitHub')}
          <Icon name="externalLink" size={14} />
        </button>
      </p>
    </SettingsField>
  </div>

  <div class="flex flex-col gap-2">
    <h3 class="font-medium text-sm text-primary">{$t('settings.about.dataTitle')}</h3>
    <p class="text-xs text-muted">{$t('settings.about.keyring')}</p>
  </div>

  <div class="flex flex-col gap-2">
    <h3 class="font-medium text-sm text-primary">{$t('settings.about.legalTitle')}</h3>
    <p class="text-xs text-muted">{$t('settings.about.license')}</p>
    <p class="text-xs text-muted">{$t('settings.about.trademark')}</p>
  </div>
</section>
