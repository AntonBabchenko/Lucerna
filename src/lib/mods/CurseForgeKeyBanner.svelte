<script lang="ts">
  // Shown in place of the search UI when source = CurseForge and no API
  // key is stored. Modrinth works anonymously; CurseForge requires an
  // API key. The CTA opens Settings → CurseForge so the user can paste
  // their key — see settingsOpen in $lib/settings/state.svelte.
  //
  // A *gate* banner (it replaces the whole search UI), so per §10 it carries no
  // dismiss × and its one CTA may be the solid .btn-warning. It uses the shared
  // Banner shell for a consistent radius/border instead of a hand-rolled recipe.

  import { t } from '$lib/i18n';
  import Banner from '$lib/ui/Banner.svelte';

  let { onOpenSettings }: { onOpenSettings: () => void } = $props();
</script>

<Banner title={$t('mods.curseforgeBanner.heading')} icon="warning" class="mx-3 my-4">
  <p class="mt-1 text-sm text-warning-text">{$t('mods.curseforgeBanner.body')}</p>
  <button type="button" class="btn-warning btn-sm mt-2" onclick={onOpenSettings}>
    {$t('mods.curseforgeBanner.openSettings')}
  </button>
</Banner>
