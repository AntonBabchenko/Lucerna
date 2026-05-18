<script lang="ts">
  import { events, type InstallPhase, type InstallProgress } from '$lib/ipc/bindings';
  import { onMount } from 'svelte';

  let progress = $state<InstallProgress | null>(null);
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
      })
      .then((u) => unlisteners.push(u));

    // Also clear on exit — covers the crash-before-menu-marker case
    // where progress.phase was 'complete' but the row would otherwise
    // linger with "click Play to launch" competing with the crash
    // banner for attention.
    events.processExited
      .listen(() => {
        progress = null;
      })
      .then((u) => unlisteners.push(u));

    return () => {
      for (const u of unlisteners) u();
      unlisteners = [];
    };
  });

  function phaseLabel(p: InstallPhase): string {
    switch (p) {
      case 'manifest':
        return 'Fetching version metadata';
      case 'forge_install':
        return 'Installing Forge';
      case 'jre':
        return 'Installing Java runtime';
      case 'libraries':
        return 'Downloading libraries';
      case 'assets':
        return 'Downloading assets';
      case 'client':
        return 'Downloading client.jar';
      case 'complete':
        return 'Install complete';
    }
  }

  function percent(done: number, total: number): number {
    if (total === 0) return 0;
    return Math.round((done / total) * 100);
  }

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }
</script>

{#if progress}
  <div class="border-t bg-neutral-50 px-4 py-1 flex items-center gap-3 text-xs">
    <span class="font-medium text-neutral-900">{phaseLabel(progress.phase)}</span>
    <span class="text-neutral-600 font-mono">
      {progress.files_done}/{progress.files_total}
      {#if progress.bytes_done && progress.bytes_done > 0}
        · {formatBytes(progress.bytes_done)}
      {/if}
    </span>
    {#if progress.phase !== 'complete'}
      <div class="flex-1 h-1 bg-neutral-200 rounded overflow-hidden">
        <div
          class="h-full bg-blue-600 transition-all"
          style="width: {percent(progress.files_done, progress.files_total)}%"
        ></div>
      </div>
    {:else}
      <span class="text-green-700">— click Play to launch</span>
    {/if}
  </div>
{/if}
