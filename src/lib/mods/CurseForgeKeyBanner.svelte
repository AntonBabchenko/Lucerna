<script lang="ts">
  // Shown in place of the search UI when source = CurseForge and no API
  // key is stored. Modrinth works anonymously; CurseForge requires an
  // API key. The CTA opens Settings → CurseForge so the user can paste
  // their key — see settingsOpen in $lib/settings/state.svelte.

  import { t } from '$lib/i18n';

  let { onOpenSettings }: { onOpenSettings: () => void } = $props();
</script>

<div
  class="p-6 bg-warning-bg border border-warning-text/30 rounded mx-3 my-4 text-sm text-warning-text"
>
  <div class="font-medium mb-1">{$t('mods.curseforgeBanner.heading')}</div>
  <p class="text-warning-text">
    {$t('mods.curseforgeBanner.body')}
  </p>
  <button type="button" class="btn-warning btn-sm" onclick={onOpenSettings}>
    {$t('mods.curseforgeBanner.openSettings')}
  </button>
</div>
