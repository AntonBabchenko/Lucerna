<script lang="ts">
  // Prominent, persistent, NON-dismissible banner shown app-wide while the launcher runs a
  // RECOVERY SESSION: the configured data folder cannot be used, so it runs on a throwaway root
  // and refuses to create or play (see data-root-gating.ts). Deliberately has no ×: per
  // DESIGN.md §10's gate-banner carve-out, a banner whose condition actively blocks other actions
  // must stay visible for as long as the condition holds.
  //
  // It says WHY (the backend's reason — "reconnect it" is a lie about a folder that is plugged in
  // and read-only) and offers the two ways forward: Storage settings, where the folder can be
  // pointed at again or detached, and Restart, for the case where the user has just fixed the
  // folder. The message text comes from `fallbackMessage`, shared with the compact-mode notice.
  //
  // Buttons (DESIGN §10): the dominant CTA is the solid `.btn-warning`, Restart its soft twin, both
  // small and UNDER the text — the content column is 580 px at the minimum window width. A failed
  // restart is shown inside the banner WITHOUT its own `role="alert"`: the banner root already is
  // one, and a nested live region would re-announce the whole banner. That is a deliberate
  // departure from ServerDiagnosisBanner, whose action-error lines do nest one (DESIGN §10).
  import { t } from '$lib/i18n';
  import type { Fallback } from '$lib/ipc/bindings';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { fallbackMessage } from './fallback-message';
  import { restartLauncherOrExplain } from './restart';
  import { openSettingsAt } from './state.svelte';

  let {
    reason,
    configuredPath,
  }: {
    reason: Fallback;
    /** The configured folder; null for the pointer reasons, whose sentence names no path. */
    configuredPath: string | null;
  } = $props();

  let restarting = $state(false);
  let restartError = $state<string | null>(null);

  const message = $derived(fallbackMessage($t, reason, configuredPath));

  async function restart(): Promise<void> {
    if (restarting) return;
    restarting = true;
    restartError = null;
    try {
      // On success this never returns: the process is replaced.
      restartError = await restartLauncherOrExplain($t);
    } finally {
      restarting = false;
    }
  }
</script>

<div
  class="bg-warning-bg border-b border-warning-text text-warning-text px-4 py-2 flex flex-col gap-2"
  data-testid="data-root-fallback-banner"
  role="alert"
>
  <div class="flex items-center gap-2">
    <Icon name="warning" class="shrink-0" />
    <span class="text-sm">{message}</span>
  </div>
  <div class="flex flex-wrap gap-2 pl-6">
    <button
      type="button"
      class="btn-warning btn-sm"
      data-testid="data-root-fallback-storage"
      onclick={() => void openSettingsAt('storage.dataLocation')}
    >
      {$t('page.dataRootFallback.storageBtn')}
    </button>
    <BusyButton
      busy={restarting}
      class="btn-warning-soft btn-sm"
      data-testid="data-root-fallback-restart"
      onclick={() => void restart()}
    >
      {$t('settings.storage.dataLocation.final.restartBtn')}
    </BusyButton>
  </div>
  {#if restartError}
    <p class="text-sm text-danger pl-6" data-testid="data-root-fallback-restart-error">
      {restartError}
    </p>
  {/if}
</div>
