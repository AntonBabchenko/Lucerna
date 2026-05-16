<script lang="ts">
  import { events, type InstallPhase, type InstallProgress } from '$lib/ipc/bindings';
  import { onMount } from 'svelte';

  let progress = $state<InstallProgress | null>(null);
  let unlisten: (() => void) | null = null;

  onMount(() => {
    events.installProgress
      .listen((event) => {
        progress = event.payload;
      })
      .then((u) => {
        unlisten = u;
      });
    return () => {
      unlisten?.();
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
  <div class="border-t bg-neutral-50 px-4 py-2 flex flex-col gap-1">
    <div class="flex items-center justify-between text-sm">
      <span class="font-medium">{phaseLabel(progress.phase)}</span>
      <span class="text-xs text-neutral-600 font-mono">
        {progress.files_done}/{progress.files_total}
        {#if progress.bytes_done && progress.bytes_done > 0}
          · {formatBytes(progress.bytes_done)}
        {/if}
      </span>
    </div>
    {#if progress.current_step}
      <p class="text-xs text-neutral-600">{progress.current_step}</p>
    {/if}
    {#if progress.phase !== 'complete'}
      <div class="h-1.5 bg-neutral-200 rounded overflow-hidden">
        <div
          class="h-full bg-blue-600 transition-all"
          style="width: {percent(progress.files_done, progress.files_total)}%"
        ></div>
      </div>
    {:else}
      <p class="text-xs text-green-700">Ready (launching arrives in slice 6)</p>
    {/if}
  </div>
{/if}
