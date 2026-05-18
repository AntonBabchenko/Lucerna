<script lang="ts">
  import { commands, type AuditEntry } from '$lib/ipc/bindings';

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let entries = $state<AuditEntry[]>([]);
  let violations = $state<AuditEntry[]>([]);
  let loading = $state(false);
  let filteringViolations = $state(false);

  async function refresh() {
    loading = true;
    try {
      const [all, vs] = await Promise.all([
        commands.networkActivity(),
        commands.networkAuditViolations(),
      ]);
      entries = all;
      violations = vs;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (open) {
      refresh();
    }
  });

  let displayed = $derived(filteringViolations ? violations : entries);

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
  <!-- Invisible click-catcher backdrop. Clicking anywhere outside the
       popover closes it. Escape on the same element also closes
       (so keyboard users can dismiss after clicking the trigger). -->
  <div
    class="fixed inset-0 z-40"
    role="button"
    tabindex="-1"
    aria-label="Close Network Activity"
    onclick={() => (open = false)}
    onkeydown={(e) => {
      if (e.key === 'Escape') open = false;
    }}
  ></div>
  <!-- Anchored bottom-left, above the sidebar's Logs/Network button row
       (sidebar is 240 px wide, bottom row sits at ~bottom-2). The popover
       grows upward from there with a 60vh ceiling. -->
  <div
    class="fixed bottom-14 left-3 w-[520px] max-h-[60vh] bg-white border rounded shadow-lg overflow-hidden flex flex-col z-50"
  >
    <header class="px-3 py-2 border-b flex items-center justify-between">
      <span class="font-semibold text-sm">Network Activity</span>
      <div class="flex items-center gap-3">
        <button class="text-xs underline" onclick={refresh} disabled={loading}>Refresh</button>
        <button
          class="text-neutral-500 hover:text-neutral-800 text-lg leading-none px-1"
          aria-label="Close"
          onclick={() => (open = false)}
        >
          ×
        </button>
      </div>
    </header>

    {#if violations.length > 0}
      <div
        class="bg-red-50 border-b border-red-300 px-3 py-2 text-xs flex items-center justify-between gap-2"
      >
        <span class="text-red-800">
          ⚠ {violations.length} request{violations.length === 1 ? '' : 's'} hit hosts outside the allowlist.
        </span>
        <button
          class="text-xs border border-red-400 rounded px-2 py-0.5 hover:bg-red-100 text-red-800 whitespace-nowrap"
          onclick={() => (filteringViolations = !filteringViolations)}
        >
          {filteringViolations ? 'Show all' : 'Show only violations'}
        </button>
      </div>
    {/if}

    <div class="overflow-auto flex-1 text-xs font-mono">
      {#if displayed.length === 0}
        <div class="px-3 py-4 text-neutral-500">
          {filteringViolations ? 'No allowlist violations.' : 'No activity yet.'}
        </div>
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
            {#each displayed as e}
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
