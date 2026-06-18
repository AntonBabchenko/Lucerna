<script lang="ts">
  import { save } from '@tauri-apps/plugin-dialog';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import type { UploadConfig } from '$lib/ipc/bindings';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  let { serverId }: { serverId: string } = $props();

  // ── seed form from existing config ─────────────────────────────────────────
  const existing = $derived(serverState.list.find((s) => s.id === serverId));
  const existingUpload = $derived(existing?.upload ?? null);

  // svelte-ignore state_referenced_locally
  let host = $state(existingUpload?.host ?? '');
  // svelte-ignore state_referenced_locally
  let port = $state(existingUpload?.port ?? 22);
  // svelte-ignore state_referenced_locally
  let user = $state(existingUpload?.user ?? '');
  let password = $state('');
  // svelte-ignore state_referenced_locally
  let remotePath = $state(existingUpload?.remote_path ?? '');

  // ── UI feedback ─────────────────────────────────────────────────────────────
  let savedVisible = $state(false);
  let saveError = $state<string | null>(null);
  let busySave = $state(false);

  let busyUpload = $state(false);
  let uploadError = $state<string | null>(null);
  let uploadedVisible = $state(false);

  let busyExport = $state(false);
  let exportError = $state<string | null>(null);
  let exportedVisible = $state(false);

  // host-key confirm dialog
  let showHostKeyConfirm = $state(false);
  let busyHostKeyTrust = $state(false);

  // ── derived ─────────────────────────────────────────────────────────────────
  const running = $derived(serverState.running(serverId));
  const progress = $derived(serverState.uploadProgressFor(serverId));

  // ── actions ─────────────────────────────────────────────────────────────────
  async function handleSave() {
    busySave = true;
    saveError = null;
    savedVisible = false;
    try {
      const cfg: UploadConfig = {
        host: host.trim(),
        port,
        user: user.trim(),
        remote_path: remotePath.trim(),
        known_host_fp: existingUpload?.known_host_fp ?? null,
      };
      const r = await serverState.setUploadConfig(serverId, cfg, password === '' ? null : password);
      if (r.status === 'ok') {
        savedVisible = true;
        await serverState.refresh();
      } else {
        saveError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busySave = false;
    }
  }

  async function handleUpload(acceptNewHostKey: boolean) {
    uploadError = null;
    uploadedVisible = false;
    serverState.clearUploadProgress(serverId);
    busyUpload = true;
    try {
      const r = await serverState.upload(serverId, acceptNewHostKey);
      if (r.status === 'ok') {
        uploadedVisible = true;
        showHostKeyConfirm = false;
      } else {
        const err = r.error as { kind: string };
        if (err?.kind === 'sftp_host_key_mismatch') {
          showHostKeyConfirm = true;
        } else {
          uploadError = formatError(r.error as Parameters<typeof formatError>[0]);
        }
      }
    } finally {
      busyUpload = false;
    }
  }

  async function handleTrustAndUpload() {
    busyHostKeyTrust = true;
    try {
      await handleUpload(true);
    } finally {
      busyHostKeyTrust = false;
    }
  }

  async function handleExport() {
    exportError = null;
    exportedVisible = false;
    const serverName = existing?.name ?? 'server';
    const dest = await save({
      defaultPath: `${serverName}.zip`,
      filters: [{ name: 'Zip', extensions: ['zip'] }],
    });
    if (!dest) return;

    busyExport = true;
    try {
      const r = await serverState.exportZip(serverId, dest);
      if (r.status === 'ok') {
        exportedVisible = true;
      } else {
        exportError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyExport = false;
    }
  }
</script>

<div class="flex flex-col gap-6">
  <!-- ── SFTP config form ─────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-3">
    <div class="grid grid-cols-[1fr_auto] gap-3">
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-host">{$t('servers.hosting.host')}</label>
        <input
          id="hosting-host"
          class="border rounded px-2 py-1 text-sm"
          bind:value={host}
          placeholder="example.com"
          autocomplete="off"
        />
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-port">{$t('servers.hosting.port')}</label>
        <input
          id="hosting-port"
          class="border rounded px-2 py-1 text-sm w-20"
          type="number"
          min={1}
          max={65535}
          bind:value={port}
        />
      </div>
    </div>

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-user">{$t('servers.hosting.user')}</label>
      <input
        id="hosting-user"
        class="border rounded px-2 py-1 text-sm"
        bind:value={user}
        autocomplete="username"
      />
    </div>

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-password"
        >{$t('servers.hosting.password')}</label
      >
      <input
        id="hosting-password"
        class="border rounded px-2 py-1 text-sm"
        type="password"
        bind:value={password}
        autocomplete="current-password"
      />
      {#if existing?.upload_password_set && !password}
        <p class="text-xs text-muted">{$t('servers.hosting.passwordStored')}</p>
      {/if}
    </div>

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-remote-path"
        >{$t('servers.hosting.remotePath')}</label
      >
      <input
        id="hosting-remote-path"
        class="border rounded px-2 py-1 text-sm"
        bind:value={remotePath}
        placeholder="/home/mc/server"
        autocomplete="off"
      />
    </div>

    <div class="flex items-center gap-3">
      <BusyButton class="btn-primary btn-sm" busy={busySave} onclick={() => void handleSave()}>
        {$t('servers.hosting.save')}
      </BusyButton>
      {#if savedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.saved')}</span>
      {/if}
    </div>
    {#if saveError}
      <p class="text-sm text-danger">{saveError}</p>
    {/if}
  </section>

  <!-- ── Upload ───────────────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    <div class="flex items-center gap-3 flex-wrap">
      <BusyButton
        class="btn-primary btn-sm"
        busy={busyUpload}
        disabled={running}
        onclick={() => void handleUpload(false)}
      >
        {#if busyUpload && progress}
          {$t('servers.hosting.uploading', { done: progress.done, total: progress.total })}
        {:else}
          {$t('servers.hosting.upload')}
        {/if}
      </BusyButton>
      {#if running}
        <span class="text-xs text-muted">{$t('servers.hosting.runningBlock')}</span>
      {/if}
      {#if uploadedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.uploaded')}</span>
      {/if}
    </div>

    {#if busyUpload && progress}
      <div class="w-full bg-muted/20 rounded-full h-1.5 overflow-hidden">
        <div
          class="bg-accent h-full transition-all"
          style="width: {progress.total > 0
            ? Math.round((progress.done / progress.total) * 100)
            : 0}%"
        ></div>
      </div>
      <p class="text-xs text-muted truncate">{progress.file}</p>
    {/if}

    {#if uploadError}
      <p class="text-sm text-danger">{uploadError}</p>
    {/if}
  </section>

  <!-- ── Host-key confirm inline ─────────────────────────────────────────── -->
  {#if showHostKeyConfirm}
    <section
      class="flex flex-col gap-3 border border-warning/40 rounded-lg p-4 bg-warning/5"
      data-testid="host-key-confirm"
    >
      <p class="text-sm font-semibold text-primary">{$t('servers.hosting.hostKeyTitle')}</p>
      <p class="text-sm text-secondary">{$t('servers.hosting.hostKeyBody')}</p>
      <div class="flex gap-2">
        <BusyButton
          class="btn-primary btn-sm"
          busy={busyHostKeyTrust}
          onclick={() => void handleTrustAndUpload()}
        >
          {$t('servers.hosting.hostKeyTrust')}
        </BusyButton>
        <button
          type="button"
          class="btn-secondary btn-sm"
          onclick={() => (showHostKeyConfirm = false)}
        >
          {$t('servers.hosting.cancel')}
        </button>
      </div>
    </section>
  {/if}

  <!-- ── Export ZIP ───────────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyExport}
        onclick={() => void handleExport()}
      >
        {$t('servers.hosting.exportZip')}
      </BusyButton>
      {#if exportedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.exported')}</span>
      {/if}
    </div>
    {#if exportError}
      <p class="text-sm text-danger">{exportError}</p>
    {/if}
  </section>
</div>
