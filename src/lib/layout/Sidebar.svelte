<script lang="ts">
  import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import InstanceConceptTooltip from '$lib/onboarding/InstanceConceptTooltip.svelte';
  import { settingsOpen } from '$lib/settings/state.svelte';
  import MicrosoftSignInButton from '$lib/accounts/MicrosoftSignInButton.svelte';

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
    running,
    installing,
    onPlay,
    onStop,
    onInstall,
    msSigningIn = $bindable(false),
    onMicrosoftSignedIn,
    onMicrosoftError,
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
    // Open the global Modpacks browser (a full-screen modal). Modpacks aren't
    // tied to the selected instance — installing a pack creates a new one — so
    // the entry point lives at the sidebar level, not as a per-instance tab.
    onOpenModpacks: () => void;
    // Launch-state inputs (moved here from the Overview pane in
    // +page.svelte). running !== null = MC is up; installing = an
    // install pipeline is in flight; otherwise the button morphs
    // between Install (not-ready) and Play (ready).
    running: { version_id: string; pid: number } | null;
    installing: boolean;
    onPlay: () => void;
    onStop: () => void;
    onInstall: () => void;
    msSigningIn?: boolean;
    onMicrosoftSignedIn?: (account: unknown) => void;
    onMicrosoftError?: (err: unknown) => void;
  } = $props();

  let showAddOfflineInput = $state(false);
  let offlineNameDraft = $state('');
</script>

<aside class="h-full bg-base border-r border-border-subtle p-3 flex flex-col gap-3 overflow-y-auto">
  <div class="font-bold text-lg text-primary">Lucerna</div>

  <div class="flex flex-col gap-1 pt-3 border-t border-border-subtle">
    <div class="text-xs uppercase tracking-wide text-muted">Account</div>
    {#if accounts.length === 0}
      <p class="text-xs text-muted">No accounts yet — add one below.</p>
    {:else}
      <select
        class="border rounded px-2 py-1 text-sm"
        value={activeAccount?.id ?? ''}
        onchange={(e) => onSelectAccount((e.currentTarget as HTMLSelectElement).value)}
      >
        {#each accounts as a}
          <option value={a.id}>{a.name} ({a.kind === 'microsoft' ? 'Microsoft' : 'offline'})</option
          >
        {/each}
      </select>
    {/if}
    <div class="flex gap-1">
      <button
        type="button"
        class="btn-secondary btn-xs flex-1"
        onclick={() => (showAddOfflineInput = !showAddOfflineInput)}
      >
        + Add offline
      </button>
      <button
        type="button"
        class="btn-secondary btn-xs flex-1"
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
            class="btn-primary btn-xs flex-1"
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
            class="btn-secondary btn-xs flex-1"
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
    <div class="mt-2">
      <MicrosoftSignInButton
        bind:signingIn={msSigningIn}
        onSignedIn={(account) => onMicrosoftSignedIn?.(account)}
        onError={(err) => onMicrosoftError?.(err)}
      />
    </div>
  </div>

  <div class="flex flex-col gap-1 pt-3 border-t border-border-subtle">
    <div class="text-xs uppercase tracking-wide text-muted flex items-center gap-1">
      <span>Instance</span>
      <InstanceConceptTooltip />
    </div>
    {#if instances.length === 0}
      <p class="text-xs text-muted">No instances yet.</p>
      <button type="button" class="btn-primary btn-xs" onclick={onOpenManage}> + Create </button>
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
          class="btn-secondary btn-xs flex-1"
          onclick={onOpenManage}
        >
          ⚙ Manage
        </button>
        <button type="button" class="btn-secondary btn-xs flex-1" onclick={onOpenMods}>
          📂 Mods
        </button>
      </div>

      {#if activeInstance}
        {#if running}
          <button type="button" data-tour="play-btn" class="btn-danger btn-lg" onclick={onStop}>
            Stop
          </button>
        {:else if activeInstance.mc_version === ''}
          <button
            type="button"
            data-tour="play-btn"
            class="btn-success btn-lg"
            disabled
            title="Pick a Minecraft version first"
          >
            Play
          </button>
        {:else if installing}
          <button type="button" data-tour="play-btn" class="btn-primary btn-lg" disabled>
            Working…
          </button>
        {:else if !activeInstance.ready}
          <!--
            Install is the only available action when an instance is not
            yet ready. Make it loud so it reads as clickable — Play and
            Install never appear at the same time, so they don't compete.
          -->
          <button type="button" data-tour="play-btn" class="btn-primary btn-lg" onclick={onInstall}>
            ↓ Install
          </button>
        {:else}
          <button type="button" data-tour="play-btn" class="btn-success btn-lg" onclick={onPlay}>
            Play
          </button>
        {/if}
      {/if}
    {/if}
  </div>

  <!--
    This action group clusters directly under the Install/Play block (no
    mt-auto) so the controls read as one unit; empty space pools at the
    bottom of the sidebar rather than between the button and these actions.
  -->
  <div class="flex flex-col gap-3 pt-3 border-t border-border-subtle">
    <!--
      Modpacks live at the sidebar level (not the per-instance tab strip)
      because installing a pack creates a NEW instance, so there's nothing
      "current instance" about the action.
    -->
    <button
      type="button"
      class="btn-secondary btn-sm flex items-center justify-center gap-1.5 hover:bg-accent-soft hover:border-accent"
      data-tour="open-modpacks"
      data-testid="sidebar-open-modpacks"
      onclick={onOpenModpacks}
    >
      📦 Browse modpacks
    </button>
    <div class="flex gap-1">
      <button type="button" class="btn-secondary btn-xs flex-1" onclick={onOpenLogs}>
        📜 Logs
      </button>
      <button
        type="button"
        class="btn-secondary btn-xs flex-1"
        aria-label="Settings"
        title="Settings"
        onclick={() => (settingsOpen.value = { tab: 'general' })}
      >
        Settings
      </button>
    </div>
  </div>
</aside>
