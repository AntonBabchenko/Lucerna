<script lang="ts">
  import { onMount } from 'svelte';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';

  // Datapack management for a server's world (S2 #27). Datapacks live under
  // runtime/<level>/datapacks/ and apply to every loader (incl. vanilla), so
  // this section is always shown. Install is a file pick → validated copy;
  // removal uses the same inline-confirm pattern as the mod list. Mutations are
  // disabled while the server runs (a live world holds files open).
  let { serverId, disabled = false }: { serverId: string; disabled?: boolean } = $props();

  let packs = $state<string[]>([]);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let busyAdd = $state(false);
  let busyRemove = $state<string | null>(null);
  let pendingRemove = $state<string | null>(null);

  async function refresh() {
    const res = await commands.serverListDatapacks(serverId);
    if (res.status === 'ok') {
      packs = res.data;
      loadError = null;
    } else {
      loadError = formatError(res.error);
    }
  }

  onMount(() => {
    void refresh();
  });

  async function addDatapack() {
    actionError = null;
    const picked = await openFile({
      multiple: false,
      filters: [{ name: 'Datapack', extensions: ['zip'] }],
    });
    if (typeof picked !== 'string') return;
    busyAdd = true;
    try {
      const res = await commands.serverInstallDatapack(serverId, picked);
      if (res.status === 'ok') {
        pushSuccess($t('servers.mods.datapackInstalled', { name: res.data }));
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyAdd = false;
    }
  }

  async function confirmRemove(filename: string) {
    busyRemove = filename;
    actionError = null;
    try {
      const res = await commands.serverRemoveDatapack(serverId, filename);
      if (res.status === 'ok') {
        pendingRemove = null;
        await refresh();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRemove = null;
    }
  }
</script>

<div class="flex flex-col gap-2">
  <div class="flex items-center justify-between gap-2">
    <h4 class="text-sm font-medium text-primary">{$t('servers.mods.datapacksTitle')}</h4>
    <BusyButton
      class="btn-secondary btn-sm"
      busy={busyAdd}
      {disabled}
      onclick={() => void addDatapack()}
      data-testid="server-datapack-add"
    >
      <Icon name="archive" size={14} />
      {$t('servers.mods.addDatapack')}
    </BusyButton>
  </div>

  {#if loadError}
    <p class="text-sm text-danger">{loadError}</p>
  {/if}
  {#if actionError}
    <p class="text-sm text-danger">{actionError}</p>
  {/if}

  {#if packs.length === 0 && !loadError}
    <p class="text-sm text-muted">{$t('servers.mods.datapacksEmpty')}</p>
  {:else}
    <div class="overflow-hidden rounded-lg border border-border-subtle">
      {#each packs as filename (filename)}
        <CardShell variant="compact-row">
          <CardMedia placeholder="package" size="sm" />
          <span class="flex-1 truncate font-mono text-xs text-primary">{filename}</span>
          {#if pendingRemove === filename}
            <span class="shrink-0 text-xs text-secondary">
              {$t('servers.mods.removeDatapackConfirm', { name: filename })}
            </span>
            <BusyButton
              class="btn-danger btn-xs"
              busy={busyRemove === filename}
              onclick={() => void confirmRemove(filename)}
            >
              {$t('servers.mods.remove')}
            </BusyButton>
            <button
              type="button"
              class="btn-ghost btn-xs"
              disabled={busyRemove === filename}
              onclick={() => (pendingRemove = null)}
            >
              {$t('common.cancel')}
            </button>
          {:else if disabled}
            <span use:tooltip={{ text: $t('servers.mods.stopToManage'), describe: false }}>
              <button
                type="button"
                class="btn-icon btn-icon-sm btn-icon-danger"
                aria-label={$t('servers.mods.remove')}
                disabled
              >
                <Icon name="trash" size={13} />
              </button>
            </span>
          {:else}
            <button
              type="button"
              class="btn-icon btn-icon-sm btn-icon-danger"
              aria-label={$t('servers.mods.remove')}
              use:tooltip={$t('servers.mods.remove')}
              onclick={() => (pendingRemove = filename)}
            >
              <Icon name="trash" size={13} />
            </button>
          {/if}
        </CardShell>
      {/each}
    </div>
  {/if}
</div>
