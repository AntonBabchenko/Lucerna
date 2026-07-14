<script lang="ts">
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import { formatHeapLabel } from '$lib/instances/heap';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { displayCore, switchTargets } from '$lib/servers/core-display';
  import SwitchCoreModal from '$lib/servers/SwitchCoreModal.svelte';
  import DeleteServerDialog from '$lib/servers/DeleteServerDialog.svelte';
  import { SavedForm } from './saved-form.svelte';
  import ServerPropertiesEditor from './ServerPropertiesEditor.svelte';

  // The merged Settings tab: two independent save sections (launcher-side
  // launch config vs the server.properties file) because they persist through
  // different commands with different failure modes — one Save button would be
  // non-atomic. Core switch and the danger zone move in unchanged.
  let { serverId }: { serverId: string } = $props();

  const server = $derived(serverState.list.find((s) => s.id === serverId));
  const running = $derived(serverState.running(serverId));

  // ── Launch section (name / memory / JVM args) ──────────────────────────────
  let name = $state('');
  let memoryMb = $state(4096);
  let jvmArgs = $state('');
  let initialized = false;

  $effect(() => {
    const s = serverState.list.find((x) => x.id === serverId);
    if (s && !initialized) {
      initialized = true;
      name = s.name;
      memoryMb = s.max_heap_mb;
      jvmArgs = s.extra_jvm_args;
    }
  });

  let launchBusy = $state(false);
  let launchError = $state<string | null>(null);
  const launchSaved = new SavedForm();
  const canSaveLaunch = $derived(name.trim().length > 0);
  const launchSig = $derived(JSON.stringify({ name, memoryMb, jvmArgs }));
  $effect(() => launchSaved.sync(launchSig));

  async function saveLaunch() {
    if (!canSaveLaunch || launchBusy) return;
    launchBusy = true;
    launchError = null;
    try {
      const r1 = await serverState.rename(serverId, name.trim());
      if (!r1.ok) {
        launchError = formatError(r1.error as Parameters<typeof formatError>[0]);
        return;
      }
      const r2 = await serverState.updateRuntimeConfig(serverId, memoryMb, jvmArgs);
      if (!r2.ok) {
        launchError = formatError(r2.error as Parameters<typeof formatError>[0]);
        return;
      }
      launchSaved.markSaved(launchSig);
    } finally {
      launchBusy = false;
    }
  }

  // ── Core switch + danger zone (moved in unchanged) ──────────────────────────
  let showSwitchCore = $state(false);
  let confirmingDelete = $state(false);
  let deleteError = $state<string | null>(null);
  let deleting = $state(false);

  async function confirmDelete() {
    confirmingDelete = false;
    deleteError = null;
    deleting = true;
    try {
      const r = await serverState.remove(serverId);
      if (!r.ok) {
        deleteError = formatError(r.error as Parameters<typeof formatError>[0]);
        return;
      }
      // Fall back to the first remaining server (or the empty state).
      serversUi.selectServer(serverState.list[0]?.id ?? null);
    } finally {
      deleting = false;
    }
  }
</script>

<div class="flex flex-col gap-6">
  <!-- Launch config -->
  <section class="flex flex-col gap-4">
    <h3 class="font-semibold">{$t('servers.settings.launchSection')}</h3>

    <div class="flex flex-col gap-1">
      <label for="sg-name" class="text-sm font-medium">{$t('servers.general.name')}</label>
      <input
        id="sg-name"
        type="text"
        maxlength="32"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={name}
      />
    </div>

    <div class="flex flex-col gap-1">
      <!-- svelte-ignore a11y_label_has_associated_control -->
      <label class="text-sm font-medium"
        >{$t('servers.general.memory')} · {formatHeapLabel(memoryMb)}</label
      >
      <MemorySlider valueMb={memoryMb} onInput={(mb) => (memoryMb = mb)} />
    </div>

    <div class="flex flex-col gap-1">
      <label for="sg-jvm" class="text-sm font-medium">{$t('servers.general.jvmArgs')}</label>
      <input
        id="sg-jvm"
        type="text"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 font-mono text-xs text-primary"
        bind:value={jvmArgs}
      />
      <p class="text-xs text-muted">{$t('servers.general.jvmArgsHint')}</p>
    </div>

    {#if running}
      <p class="text-xs text-warning-text">{$t('servers.general.restartToApply')}</p>
    {/if}

    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-primary btn-sm"
        busy={launchBusy}
        disabled={!canSaveLaunch}
        data-testid="settings-launch-save"
        onclick={() => void saveLaunch()}
      >
        {$t('servers.general.save')}
      </BusyButton>
      {#if launchSaved.saved}
        <span class="text-xs text-success">{$t('servers.general.saved')}</span>
      {/if}
      {#if launchError}
        <span class="text-xs text-danger">{launchError}</span>
      {/if}
    </div>
  </section>

  <!-- server.properties -->
  <section class="border-t border-border-subtle pt-4">
    <ServerPropertiesEditor {serverId} {running} />
  </section>

  {#if server}
    <!-- Server core -->
    <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
      <h3 class="font-semibold mb-1">{$t('servers.core.sectionTitle')}</h3>
      <p class="text-sm text-secondary">
        {$t('servers.core.current')}: <span class="font-medium">{displayCore(server.loader)}</span>
        {#if server.loader_version}
          <span class="text-muted">({server.loader_version})</span>
        {/if}
      </p>
      {#if switchTargets(server.loader).length > 0}
        <div>
          <button
            type="button"
            class="btn-secondary btn-sm"
            disabled={server.running}
            onclick={() => (showSwitchCore = true)}
          >
            {$t('servers.core.switchButton')}
          </button>
          {#if server.running}
            <p class="text-xs text-muted mt-1">{$t('servers.core.stopToSwitch')}</p>
          {/if}
        </div>
      {/if}
    </section>
  {/if}

  <div class="border-t border-border-subtle pt-4 flex flex-col gap-2">
    <span class="text-sm font-medium text-danger">{$t('servers.general.dangerTitle')}</span>
    <span
      class="inline-flex self-start"
      use:tooltip={{
        text: running ? $t('servers.delete.runningBlock') : $t('servers.delete.trigger'),
        describe: false,
      }}
    >
      <button
        type="button"
        class="btn-danger btn-sm flex items-center gap-1.5"
        disabled={running || deleting}
        data-testid="server-delete-trigger"
        onclick={() => (confirmingDelete = true)}
      >
        <Icon name="trash" size={14} />
        {$t('servers.delete.trigger')}
      </button>
    </span>
    {#if deleteError}
      <span class="text-xs text-danger" role="alert">{deleteError}</span>
    {/if}
  </div>
</div>

{#if showSwitchCore && server}
  <SwitchCoreModal
    serverId={server.id}
    currentCore={server.loader}
    onClose={() => (showSwitchCore = false)}
  />
{/if}

{#if confirmingDelete}
  {@const s = serverState.list.find((x) => x.id === serverId)}
  <DeleteServerDialog
    serverName={s?.name ?? serverId}
    onCancel={() => (confirmingDelete = false)}
    onConfirm={() => void confirmDelete()}
  />
{/if}
