<script lang="ts">
  import { onMount } from 'svelte';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { commands } from '$lib/ipc/bindings';
  import type { UploadConfig, UploadAuthMethod } from '$lib/ipc/bindings';
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

  // ── SFTP auth method (#28) — loaded from the S4 sidecar ─────────────────────
  let authMethod = $state<UploadAuthMethod>('password');
  let privateKeyPath = $state('');

  // ── automatic-backup policy (#29) ───────────────────────────────────────────
  let backupEnabled = $state(false);
  let backupIntervalMinutes = $state(60);
  let busyBackupPolicy = $state(false);
  let backupPolicySaved = $state(false);
  let backupPolicyError = $state<string | null>(null);

  onMount(async () => {
    const auth = await commands.serverGetUploadAuth(serverId);
    if (auth.status === 'ok') {
      authMethod = auth.data.method ?? 'password';
      privateKeyPath = auth.data.private_key_path ?? '';
    }
    const policy = await commands.serverBackupPolicyGet(serverId);
    if (policy.status === 'ok') {
      backupEnabled = policy.data.enabled ?? false;
      const interval = policy.data.interval_minutes ?? 0;
      if (interval > 0) backupIntervalMinutes = interval;
    }
  });

  async function handleSaveBackupPolicy() {
    busyBackupPolicy = true;
    backupPolicyError = null;
    backupPolicySaved = false;
    try {
      const r = await commands.serverBackupPolicySet(serverId, {
        enabled: backupEnabled,
        interval_minutes: Math.max(1, Math.round(backupIntervalMinutes)),
        last_run_unix_ms: 0,
      });
      if (r.status === 'ok') {
        backupPolicySaved = true;
      } else {
        backupPolicyError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyBackupPolicy = false;
    }
  }

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

  // host-key confirm dialog (#24): shown on first connect (with the fingerprint
  // to verify) AND on a later mismatch.
  let showHostKeyConfirm = $state(false);
  let busyHostKeyTrust = $state(false);
  let hostKeyFingerprint = $state<string | null>(null);
  let hostKeyIsFirstConnect = $state(false);

  // ── derived ─────────────────────────────────────────────────────────────────
  const running = $derived(serverState.running(serverId));
  const progress = $derived(serverState.uploadProgressFor(serverId));

  // ── validation (#25) ─────────────────────────────────────────────────────────
  const hostValid = $derived(host.trim().length > 0);
  const userValid = $derived(user.trim().length > 0);
  const keyValid = $derived(authMethod !== 'key' || privateKeyPath.trim().length > 0);
  const formValid = $derived(hostValid && userValid && keyValid);

  // The password is only meaningful for password auth, or as an optional key
  // passphrase. Treat it as a secret to persist when present.
  const secretToStore = $derived(password === '' ? null : password);

  // ── actions ─────────────────────────────────────────────────────────────────
  async function pickPrivateKey() {
    const picked = await open({ multiple: false, directory: false });
    if (typeof picked === 'string') privateKeyPath = picked;
  }

  async function handleSave() {
    if (!formValid) return;
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
      const auth = await commands.serverSetUploadAuth(serverId, {
        method: authMethod,
        private_key_path: authMethod === 'key' ? privateKeyPath.trim() : null,
      });
      if (auth.status !== 'ok') {
        saveError = formatError(auth.error as Parameters<typeof formatError>[0]);
        return;
      }
      const r = await serverState.setUploadConfig(serverId, cfg, secretToStore);
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

  /** Run the actual upload; `acceptNewHostKey` trusts a new/changed host key. */
  async function doUpload(acceptNewHostKey: boolean) {
    uploadError = null;
    uploadedVisible = false;
    serverState.clearUploadProgress(serverId);
    busyUpload = true;
    try {
      const r = await serverState.upload(serverId, acceptNewHostKey);
      if (r.status === 'ok') {
        uploadedVisible = true;
        showHostKeyConfirm = false;
        await serverState.refresh();
      } else {
        const err = r.error as { kind: string; got?: string };
        if (err?.kind === 'sftp_host_key_mismatch') {
          // Changed key: surface the new fingerprint for the user to weigh.
          hostKeyFingerprint = err.got ?? null;
          hostKeyIsFirstConnect = false;
          showHostKeyConfirm = true;
        } else {
          uploadError = formatError(r.error as Parameters<typeof formatError>[0]);
        }
      }
    } finally {
      busyUpload = false;
    }
  }

  /** First click: on first connect, fetch + show the fingerprint to verify
   *  BEFORE uploading (#24). Once a host key is trusted, upload directly. */
  async function handleUpload() {
    if (existingUpload?.known_host_fp) {
      await doUpload(false);
      return;
    }
    uploadError = null;
    busyUpload = true;
    try {
      const r = await commands.serverHostKeyPreview(serverId);
      if (r.status === 'ok') {
        hostKeyFingerprint = r.data.fingerprint;
        hostKeyIsFirstConnect = true;
        showHostKeyConfirm = true;
      } else {
        uploadError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyUpload = false;
    }
  }

  async function handleTrustAndUpload() {
    busyHostKeyTrust = true;
    try {
      await doUpload(true);
    } finally {
      busyHostKeyTrust = false;
    }
  }

  async function handleExport() {
    if (running) return;
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
          class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
          class:border-danger={!hostValid && host.length > 0}
          bind:value={host}
          placeholder="example.com"
          autocomplete="off"
        />
        {#if !hostValid}
          <p class="text-xs text-muted">{$t('servers.hosting.hostRequired')}</p>
        {/if}
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-port">{$t('servers.hosting.port')}</label>
        <input
          id="hosting-port"
          class="h-8 w-20 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
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
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        class:border-danger={!userValid && user.length > 0}
        bind:value={user}
        autocomplete="username"
      />
      {#if !userValid}
        <p class="text-xs text-muted">{$t('servers.hosting.userRequired')}</p>
      {/if}
    </div>

    <!-- ── Auth method (#28) ─────────────────────────────────────────────── -->
    <div class="flex flex-col gap-1">
      <span class="text-xs text-muted">{$t('servers.hosting.authMethod')}</span>
      <div class="flex gap-4 text-sm">
        <label class="flex items-center gap-1.5">
          <input type="radio" value="password" bind:group={authMethod} />
          {$t('servers.hosting.authPassword')}
        </label>
        <label class="flex items-center gap-1.5">
          <input type="radio" value="key" bind:group={authMethod} />
          {$t('servers.hosting.authKey')}
        </label>
      </div>
    </div>

    {#if authMethod === 'key'}
      <div class="flex flex-col gap-1">
        <label class="text-xs text-muted" for="hosting-key-path"
          >{$t('servers.hosting.privateKeyPath')}</label
        >
        <div class="flex gap-2">
          <input
            id="hosting-key-path"
            class="h-8 flex-1 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
            class:border-danger={!keyValid && privateKeyPath.length > 0}
            bind:value={privateKeyPath}
            placeholder="~/.ssh/id_ed25519"
            autocomplete="off"
          />
          <button type="button" class="btn-secondary btn-sm" onclick={() => void pickPrivateKey()}>
            {$t('servers.hosting.browse')}
          </button>
        </div>
        {#if !keyValid}
          <p class="text-xs text-muted">{$t('servers.hosting.keyPathRequired')}</p>
        {/if}
      </div>
    {/if}

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-password">
        {authMethod === 'key' ? $t('servers.hosting.passphrase') : $t('servers.hosting.password')}
      </label>
      <input
        id="hosting-password"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        type="password"
        bind:value={password}
        autocomplete="current-password"
      />
      {#if existing?.upload_password_set && !password}
        <p class="text-xs text-muted">
          {authMethod === 'key'
            ? $t('servers.hosting.passphraseStored')
            : $t('servers.hosting.passwordStored')}
        </p>
      {/if}
    </div>

    <div class="flex flex-col gap-1">
      <label class="text-xs text-muted" for="hosting-remote-path"
        >{$t('servers.hosting.remotePath')}</label
      >
      <input
        id="hosting-remote-path"
        class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={remotePath}
        placeholder="/home/mc/server"
        autocomplete="off"
      />
    </div>

    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-primary btn-sm"
        busy={busySave}
        disabled={!formValid}
        onclick={() => void handleSave()}
      >
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
        disabled={running || !formValid}
        onclick={() => void handleUpload()}
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

  <!-- ── Host-key confirm inline (#24) ───────────────────────────────────── -->
  {#if showHostKeyConfirm}
    <section
      class="flex flex-col gap-3 border border-warning-text rounded-lg p-4 bg-warning-bg"
      data-testid="host-key-confirm"
    >
      <p class="text-sm font-semibold text-primary">
        {hostKeyIsFirstConnect
          ? $t('servers.hosting.hostKeyFirstTitle')
          : $t('servers.hosting.hostKeyTitle')}
      </p>
      <p class="text-sm text-secondary">
        {hostKeyIsFirstConnect
          ? $t('servers.hosting.hostKeyFirstBody')
          : $t('servers.hosting.hostKeyBody')}
      </p>
      {#if hostKeyFingerprint}
        <div class="flex flex-col gap-1">
          <span class="text-xs text-muted">{$t('servers.hosting.hostKeyFingerprint')}</span>
          <code class="text-xs break-all font-mono bg-muted/20 rounded px-2 py-1"
            >{hostKeyFingerprint}</code
          >
        </div>
      {/if}
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

  <!-- ── Automatic backups (#29) ──────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    <p class="text-sm font-semibold text-primary">{$t('servers.backups.autoTitle')}</p>
    <p class="text-xs text-muted">{$t('servers.backups.autoHint')}</p>
    <label class="flex items-center gap-2 text-sm">
      <input type="checkbox" bind:checked={backupEnabled} />
      {$t('servers.backups.autoEnable')}
    </label>
    {#if backupEnabled}
      <div class="flex items-center gap-2 text-sm">
        <label for="hosting-backup-interval">{$t('servers.backups.autoInterval')}</label>
        <input
          id="hosting-backup-interval"
          class="h-8 w-20 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
          type="number"
          min={1}
          max={1440}
          bind:value={backupIntervalMinutes}
        />
        <span>{$t('servers.backups.autoIntervalUnit')}</span>
      </div>
    {/if}
    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyBackupPolicy}
        onclick={() => void handleSaveBackupPolicy()}
      >
        {$t('servers.backups.autoSave')}
      </BusyButton>
      {#if backupPolicySaved}
        <span class="text-sm text-success">{$t('servers.backups.autoSaved')}</span>
      {/if}
    </div>
    {#if backupPolicyError}
      <p class="text-sm text-danger">{backupPolicyError}</p>
    {/if}
  </section>

  <!-- ── Export ZIP ───────────────────────────────────────────────────────── -->
  <section class="flex flex-col gap-2 border-t border-border-subtle pt-4">
    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyExport}
        disabled={running}
        onclick={() => void handleExport()}
      >
        {$t('servers.hosting.exportZip')}
      </BusyButton>
      {#if running}
        <span class="text-xs text-muted">{$t('servers.hosting.exportRunningBlock')}</span>
      {/if}
      {#if exportedVisible}
        <span class="text-sm text-success">{$t('servers.hosting.exported')}</span>
      {/if}
    </div>
    {#if exportError}
      <p class="text-sm text-danger">{exportError}</p>
    {/if}
  </section>
</div>
