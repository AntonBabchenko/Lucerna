<script lang="ts">
  import { commands, type AuditEntry } from '$lib/ipc/bindings';

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let entries = $state<AuditEntry[]>([]);
  let loading = $state(false);

  async function refresh() {
    loading = true;
    try {
      entries = await commands.networkActivity();
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (open) {
      refresh();
    }
  });

  function formatTs(ms: number | null): string {
    if (ms === null) return '—';
    return new Date(ms).toISOString().slice(11, 19);
  }

  function formatBytes(n: number | null): string {
    if (n === null) return '—';
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(2)} MB`;
  }
</script>

{#if open}
  <div
    class="absolute right-2 top-12 w-[520px] max-h-[60vh] bg-white border rounded shadow-lg overflow-hidden flex flex-col"
  >
    <header class="px-3 py-2 border-b flex items-center justify-between">
      <span class="font-semibold text-sm">Network Activity</span>
      <button class="text-xs underline" onclick={refresh} disabled={loading}>Refresh</button>
    </header>

    <div class="overflow-auto flex-1 text-xs font-mono">
      {#if entries.length === 0}
        <div class="px-3 py-4 text-neutral-500">No activity yet.</div>
      {:else}
        <table class="w-full">
          <thead class="bg-neutral-50 sticky top-0">
            <tr>
              <th class="text-left px-2 py-1">Time</th>
              <th class="text-left px-2 py-1">Method</th>
              <th class="text-left px-2 py-1">URL</th>
              <th class="text-right px-2 py-1">Bytes</th>
              <th class="text-right px-2 py-1">Status</th>
              <th class="text-left px-2 py-1">From</th>
            </tr>
          </thead>
          <tbody>
            {#each entries as e}
              <tr class="border-b border-neutral-100">
                <td class="px-2 py-1">{formatTs(e.ts)}</td>
                <td class="px-2 py-1">{e.method}</td>
                <td class="px-2 py-1 truncate max-w-[200px]" title={e.url}>{e.url}</td>
                <td class="px-2 py-1 text-right">{formatBytes(e.bytes)}</td>
                <td class="px-2 py-1 text-right">{e.status ?? '—'}</td>
                <td class="px-2 py-1">{e.initiator}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </div>
{/if}
