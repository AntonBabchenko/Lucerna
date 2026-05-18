<script lang="ts">
  import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';

  let {
    accounts,
    activeAccount,
    instances,
    activeInstance,
    violationsCount,
    onSelectAccount,
    onRemoveAccount,
    onAddOffline,
    onSelectInstance,
    onOpenManage,
    onOpenMods,
    onOpenLogs,
    onOpenNetwork,
  }: {
    accounts: Account[];
    activeAccount: Account | null;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    violationsCount: number;
    onSelectAccount: (id: string) => void;
    onRemoveAccount: () => void;
    onAddOffline: (name: string) => void;
    onSelectInstance: (id: string) => void;
    onOpenManage: () => void;
    onOpenMods: () => void;
    onOpenLogs: () => void;
    onOpenNetwork: () => void;
  } = $props();

  let showAddOfflineInput = $state(false);
  let offlineNameDraft = $state('');
</script>

<aside
  class="h-full bg-neutral-50 border-r border-neutral-200 p-3 flex flex-col gap-3 overflow-y-auto"
>
  <div class="font-bold text-lg text-neutral-900">FTlauncher</div>

  <div class="flex flex-col gap-1">
    <div class="text-xs uppercase tracking-wide text-neutral-500">Account</div>
    {#if accounts.length === 0}
      <p class="text-xs text-neutral-500">No accounts yet — add one below.</p>
    {:else}
      <select
        class="border rounded px-2 py-1 text-sm"
        value={activeAccount?.id ?? ''}
        onchange={(e) => onSelectAccount((e.currentTarget as HTMLSelectElement).value)}
      >
        {#each accounts as a}
          <option value={a.id}>{a.name} (offline)</option>
        {/each}
      </select>
    {/if}
    <div class="flex gap-1">
      <button
        type="button"
        class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white"
        onclick={() => (showAddOfflineInput = !showAddOfflineInput)}
      >
        + Add offline
      </button>
      <button
        type="button"
        class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white disabled:opacity-40"
        disabled={!activeAccount}
        onclick={onRemoveAccount}
      >
        Remove
      </button>
    </div>
    {#if showAddOfflineInput}
      <div class="flex flex-col gap-1 mt-1">
        <input
          class="border rounded px-2 py-1 text-sm"
          placeholder="Player name"
          maxlength="16"
          bind:value={offlineNameDraft}
        />
        <div class="flex gap-1">
          <button
            type="button"
            class="flex-1 border rounded px-2 py-1.5 text-xs bg-blue-600 text-white hover:bg-blue-700"
            onclick={() => {
              onAddOffline(offlineNameDraft.trim());
              showAddOfflineInput = false;
              offlineNameDraft = '';
            }}
          >
            Add
          </button>
          <button
            type="button"
            class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white"
            onclick={() => {
              showAddOfflineInput = false;
              offlineNameDraft = '';
            }}
          >
            Cancel
          </button>
        </div>
      </div>
    {/if}
    <p class="text-xs text-neutral-500 italic mt-1">
      Microsoft account login deferred — revisit after v0.5.0.
    </p>
  </div>

  <div class="flex flex-col gap-1">
    <div class="text-xs uppercase tracking-wide text-neutral-500">Instance</div>
    {#if instances.length === 0}
      <p class="text-xs text-neutral-500">No instances yet.</p>
      <button
        type="button"
        class="border rounded px-2 py-1.5 text-xs bg-blue-600 text-white hover:bg-blue-700"
        onclick={onOpenManage}
      >
        + Create
      </button>
    {:else}
      <select
        class="border rounded px-2 py-1 text-sm"
        value={activeInstance?.id ?? ''}
        onchange={(e) => onSelectInstance((e.currentTarget as HTMLSelectElement).value)}
      >
        {#each instances as i}
          <option value={i.id}>
            {i.ready ? '✓' : '↓'}
            {i.name} · {displayLoader(i.loader)}
            {i.mc_version || '(pick MC)'}
          </option>
        {/each}
      </select>
      <div class="flex gap-1">
        <button
          type="button"
          class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white"
          onclick={onOpenManage}
        >
          ⚙ Manage
        </button>
        <button
          type="button"
          class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white"
          onclick={onOpenMods}
        >
          📂 Mods
        </button>
      </div>
    {/if}
  </div>

  <div class="mt-auto pt-2 border-t border-neutral-200 flex gap-1">
    <button
      type="button"
      class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white"
      onclick={onOpenLogs}
    >
      📜 Logs
    </button>
    <button
      type="button"
      class="flex-1 border rounded px-2 py-1.5 text-xs hover:bg-white relative"
      onclick={onOpenNetwork}
    >
      🌐 Network
      {#if violationsCount > 0}
        <span
          class="absolute top-0.5 right-1 w-1.5 h-1.5 bg-red-600 rounded-full"
          aria-label="{violationsCount} allowlist violations"
        ></span>
      {/if}
    </button>
  </div>
</aside>
