<script lang="ts">
  // About section of Settings — required for Minecraft Usage Guidelines
  // compliance (verbatim disclaimer + identity of who ships this).
  // The disclaimer text lives in `./disclaimer.ts` so the test file and
  // the panel never disagree on the exact string.
  import pkg from '../../../package.json' with { type: 'json' };
  import { REPO_URL } from './disclaimer';
  import { t } from '$lib/i18n';

  const version = pkg.version;

  function openRepo() {
    // Lazy import keeps the bundle slim and matches the pattern used in
    // CurseForgeKeyForm.svelte / ModDetailDrawer.svelte.
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(REPO_URL));
  }
</script>

<section class="space-y-3 text-sm selectable">
  <h3 class="text-base font-semibold text-primary">{$t('settings.about.title')}</h3>
  <p class="font-medium text-primary">Lucerna v{version}</p>
  <p class="text-secondary">{$t('settings.about.disclaimer')}</p>
  <p>
    <button
      type="button"
      class="btn-tertiary"
      title={REPO_URL}
      aria-label={$t('settings.about.openRepoLabel', { url: REPO_URL })}
      onclick={openRepo}
    >
      {$t('settings.about.viewOnGitHub')}
    </button>
  </p>
  <p class="text-xs text-muted">
    {$t('settings.about.license')}
  </p>
  <p class="text-xs text-muted">
    {$t('settings.about.trademark')}
  </p>
  <p class="text-xs text-muted">
    {$t('settings.about.keyring')}
  </p>
</section>
