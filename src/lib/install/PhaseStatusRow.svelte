<script lang="ts">
  import { t } from '$lib/i18n';
  import { formatSize } from '$lib/format/size';
  import {
    events,
    type InstallPhase,
    type InstallProgress,
    type ModInstallProgress,
  } from '$lib/ipc/bindings';
  import { get } from 'svelte/store';
  import { onMount } from 'svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  let progress = $state<InstallProgress | null>(null);
  let modProgress = $state<ModInstallProgress | null>(null);
  let unlisteners: Array<() => void> = [];

  onMount(() => {
    events.installProgress
      .listen((event) => {
        progress = event.payload;
      })
      .then((u) => unlisteners.push(u));

    // Clear install progress when MC starts — the install context is
    // over once a process is spawned.
    events.processSpawned
      .listen(() => {
        progress = null;
        modProgress = null;
      })
      .then((u) => unlisteners.push(u));

    // Also clear on exit — covers the crash-before-menu-marker case
    // where progress.phase was 'complete' but the row would otherwise
    // linger with "click Play to launch" competing with the crash
    // banner for attention.
    events.processExited
      .listen(() => {
        progress = null;
        modProgress = null;
      })
      .then((u) => unlisteners.push(u));

    // Mod install pipeline — streamed phases per mod (downloading /
    // verifying / copying). Sequence length isn't known up front
    // (mods_install_with_deps doesn't emit a "start" event), so we
    // just show the phase string. Counter dropped for v1.
    events.modInstallProgress
      .listen((event) => {
        modProgress = event.payload;
      })
      .then((u) => unlisteners.push(u));

    // Clear mod state once the per-mod install completes — the next
    // modInstallProgress event will refresh it for the next mod in the
    // dep sequence (if any).
    events.modInstalled
      .listen(() => {
        modProgress = null;
      })
      .then((u) => unlisteners.push(u));

    // Clear on failure so the row doesn't linger after an error.
    events.modInstallFailed
      .listen(() => {
        modProgress = null;
      })
      .then((u) => unlisteners.push(u));

    return () => {
      for (const u of unlisteners) u();
      unlisteners = [];
    };
  });

  function phaseLabel(p: InstallPhase): string {
    const tr = get(t);
    switch (p) {
      case 'manifest':
        return tr('install.phase.manifest');
      case 'forge_install':
        return tr('install.phase.forge_install');
      case 'jre':
        return tr('install.phase.jre');
      case 'libraries':
        return tr('install.phase.libraries');
      case 'assets':
        return tr('install.phase.assets');
      case 'client':
        return tr('install.phase.client');
      case 'complete':
        return tr('install.phase.complete');
    }
  }

  function modPhaseLabel(p: ModInstallProgress['phase']): string {
    const tr = get(t);
    switch (p) {
      case 'downloading':
        return tr('install.modPhase.downloading');
      case 'verifying':
        return tr('install.modPhase.verifying');
      case 'copying':
        return tr('install.modPhase.copying');
    }
  }

  function percent(done: number, total: number): number {
    if (total === 0) return 0;
    return Math.round((done / total) * 100);
  }
</script>

{#if modProgress}
  <div class="border-t bg-base px-4 py-1 flex items-center gap-3 text-xs">
    <Spinner size="sm" />
    <span class="font-medium text-primary">{modPhaseLabel(modProgress.phase)}</span>
    {#if modProgress.phase === 'downloading' && modProgress.bytes_total && modProgress.bytes_total > 0}
      <span class="text-secondary font-mono">
        {percent(modProgress.bytes_done ?? 0, modProgress.bytes_total)}%
      </span>
      <div class="flex-1 h-1 bg-subtle rounded overflow-hidden">
        <div
          class="h-full bg-accent transition-all"
          style="width: {percent(modProgress.bytes_done ?? 0, modProgress.bytes_total)}%"
        ></div>
      </div>
    {/if}
  </div>
{:else if progress}
  <div class="border-t bg-base px-4 py-1 flex items-center gap-3 text-xs">
    {#if progress.phase !== 'complete'}<Spinner size="sm" />{/if}
    <span class="font-medium text-primary">{phaseLabel(progress.phase)}</span>
    <span class="text-secondary font-mono">
      {progress.files_done}/{progress.files_total}
      {#if progress.bytes_done && progress.bytes_done > 0}
        · {formatSize($t, progress.bytes_done)}
      {/if}
    </span>
    {#if progress.phase !== 'complete'}
      <div class="flex-1 h-1 bg-subtle rounded overflow-hidden">
        <div
          class="h-full bg-accent transition-all"
          style="width: {percent(progress.files_done, progress.files_total)}%"
        ></div>
      </div>
    {:else}
      <span class="text-success">{$t('install.clickToLaunch')}</span>
    {/if}
  </div>
{/if}
