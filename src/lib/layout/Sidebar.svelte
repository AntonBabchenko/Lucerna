<script lang="ts">
  import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import InstanceConceptTooltip from '$lib/onboarding/InstanceConceptTooltip.svelte';
  import { settingsOpen } from '$lib/settings/state.svelte';

  let {
    accounts,
    activeAccount,
    instances,
    activeInstance,
    onSelectAccount,
    onRemoveAccount,
    onAddOffline,
    onSelectInstance,
    onOpenManage,
    onOpenMods,
    onOpenLogs,
    onOpenModpacks,
    modpacksActive,
    running,
    installing,
    onPlay,
    onStop,
    onInstall,
  }: {
    accounts: Account[];
    activeAccount: Account | null;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    onSelectAccount: (id: string) => void;
    onRemoveAccount: () => void;
    onAddOffline: (name: string) => void;
    onSelectInstance: (id: string) => void;
    onOpenManage: () => void;
    onOpenMods: () => void;
    onOpenLogs: () => void;
    // Switch the right pane between the per-instance MainTabs view and
    // the global Modpacks browser. Modpacks aren't tied to the selected
    // instance — installing a pack creates a new one — so they live at
    // the sidebar level rather than as a 4th instance tab.
    onOpenModpacks: () => void;
    modpacksActive: boolean;
    // Launch-state inputs (moved here from the Overview pane in
    // +page.svelte). running !== null = MC is up; installing = an
    // install pipeline is in flight; otherwise the button morphs
    // between Install (not-ready) and Play (ready).
    running: { version_id: string; pid: number } | null;
    installing: boolean;
    onPlay: () => void;
    onStop: () => void;
    onInstall: () => void;
  } = $props();

  let showAddOfflineInput = $state(false);
  let offlineNameDraft = $state('');
</script>

<aside
  class="h-full bg-neutral-50 border-r border-neutral-200 p-3 flex flex-col gap-3 overflow-y-auto"
>
  <div class="font-bold text-lg text-neutral-900">FTlauncher</div>

  <div class="flex flex-col gap-1 pt-3 border-t border-neutral-200">
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

  <div class="flex flex-col gap-1 pt-3 border-t border-neutral-200">
    <div class="text-xs uppercase tracking-wide text-neutral-500 flex items-center gap-1">
      <span>Instance</span>
      <InstanceConceptTooltip />
    </div>
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
        data-tour="instance-picker"
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
          data-tour="manage-btn"
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

      {#if activeInstance}
        {#if running}
          <button
            type="button"
            data-tour="play-btn"
            class="bg-red-600 hover:bg-red-700 text-white rounded px-3 py-2 text-sm font-semibold"
            onclick={onStop}
          >
            Stop
          </button>
        {:else if activeInstance.mc_version === ''}
          <button
            type="button"
            data-tour="play-btn"
            class="bg-neutral-300 text-neutral-600 rounded px-3 py-2 text-sm font-semibold cursor-not-allowed"
            disabled
            title="Pick a Minecraft version first"
          >
            Play
          </button>
        {:else if installing}
          <button
            type="button"
            data-tour="play-btn"
            class="bg-blue-400 text-white rounded px-3 py-2 text-sm font-semibold cursor-not-allowed"
            disabled
          >
            Working…
          </button>
        {:else if !activeInstance.ready}
          <button
            type="button"
            data-tour="play-btn"
            class="bg-blue-600 hover:bg-blue-700 text-white rounded px-3 py-2 text-sm font-semibold"
            onclick={onInstall}
          >
            Install
          </button>
        {:else}
          <button
            type="button"
            data-tour="play-btn"
            class="bg-green-600 hover:bg-green-700 text-white rounded px-3 py-2 text-sm font-semibold"
            onclick={onPlay}
          >
            Play
          </button>
        {/if}
      {/if}
    {/if}
  </div>

  <div class="mt-auto flex flex-col gap-3 pt-3 border-t border-neutral-200">
    <!--
      Modpacks live at the sidebar level (not the per-instance tab strip)
      because installing a pack creates a NEW instance, so there's nothing
      "current instance" about the action.
    -->
    <button
      type="button"
      class="border rounded px-2 py-2 text-sm hover:bg-blue-50 hover:border-blue-300 flex items-center justify-center gap-1.5"
      class:bg-blue-50={modpacksActive}
      class:border-blue-400={modpacksActive}
      class:text-blue-800={modpacksActive}
      class:font-medium={modpacksActive}
      data-tour="open-modpacks"
      data-testid="sidebar-open-modpacks"
      title={modpacksActive ? 'Click to return to the instance view' : undefined}
      onclick={onOpenModpacks}
    >
      {#if modpacksActive}
        ← Back to instance
      {:else}
        📦 Browse modpacks
      {/if}
    </button>
    <div class="flex justify-end gap-1">
      <button
        type="button"
        class="w-9 h-9 inline-flex items-center justify-center border rounded text-base hover:bg-white"
        aria-label="Logs"
        title="Logs"
        onclick={onOpenLogs}
      >
        📜
      </button>
      <button
        type="button"
        class="w-9 h-9 inline-flex items-center justify-center border rounded text-base hover:bg-white"
        aria-label="Settings"
        title="Settings"
        onclick={() => (settingsOpen.value = { tab: 'curseforge' })}
      >
        ⚙
      </button>
    </div>
  </div>
</aside>
