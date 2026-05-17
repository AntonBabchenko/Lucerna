<script lang="ts">
  import {
    commands,
    events,
    type Account,
    type CrashReport,
    type Error as IpcError,
    type InstanceWithStatus,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import NetworkPopover from '$lib/network/NetworkPopover.svelte';
  import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';
  import { onMount, untrack } from 'svelte';

  let accounts = $state<Account[]>([]);
  let activeAccount = $state<Account | null>(null);
  let showAddOfflineInput = $state(false);
  let offlineNameDraft = $state('');
  let offlineNameError = $state<string | null>(null);
  let listAccountsError = $state<string | null>(null);
  let removeError = $state<string | null>(null);
  let networkOpen = $state(false);

  // Vanilla MC version manifest, still fetched on mount and passed
  // through to ManageInstancesModal where it powers the MC version
  // picker. Home no longer renders a version dropdown — see Manage.
  let versions = $state<VersionEntry[]>([]);
  let versionsError = $state<string | null>(null);

  let instances = $state<InstanceWithStatus[]>([]);
  let activeInstance = $state<InstanceWithStatus | null>(null);
  let instancesError = $state<string | null>(null);

  let manageOpen = $state(false);

  let installing = $state(false);
  let installError = $state<string | null>(null);
  let running = $state<{ pid: number; version_id: string } | null>(null);
  let exited = $state<{ code: number; log_path: string } | null>(null);
  let spawnUnlisten: (() => void) | null = null;
  let exitUnlisten: (() => void) | null = null;

  let logsOpen = $state(false);
  let logsInitialPath = $state<string | null>(null);
  let crashReport = $state<CrashReport | null>(null);
  let modsError = $state<string | null>(null);
  let violationsCount = $state(0);

  // Whenever the active instance changes, clear per-instance error banners.
  // They refer to the previously-active instance and confuse the user when
  // they switch context (e.g. fix one instance's setup by switching to
  // another, only to still see the old error).
  let lastActiveId: string | null = null;
  $effect(() => {
    const newId = activeInstance?.id ?? null;
    untrack(() => {
      if (newId !== lastActiveId) {
        lastActiveId = newId;
        installError = null;
        modsError = null;
        exited = null;
        crashReport = null;
      }
    });
  });

  function errorMessage(e: IpcError): string {
    switch (e.kind) {
      case 'network':
        return `Network error fetching ${e.url}: ${e.details}`;
      case 'hash_mismatch':
        return `Hash mismatch for ${e.path}`;
      case 'java_spawn':
        return `Java spawn failed: ${e.details}`;
      case 'already_running':
        return 'Minecraft is already running';
      case 'account_not_set':
        return 'Account not set — enter your name first';
      case 'unknown_version':
        return `Version ${e.id} not found in manifest`;
      case 'unsupported_platform':
        return `Unsupported platform: ${e.os}/${e.arch}`;
      case 'loader_unavailable':
        return `${e.loader} does not support Minecraft ${e.mc_version}`;
      case 'last_instance':
        return 'Cannot delete the last instance — at least one must remain';
      case 'no_version_selected':
        return 'Pick a Minecraft version first';
      case 'instance_not_found':
        return `Instance ${e.id} not found`;
      case 'io':
        return `IO error at ${e.path}: ${e.details}`;
      case 'forge_promotions_unavailable':
        return `Forge promotions feed for ${e.flavor} is unavailable — versions will not be marked recommended`;
      case 'forge_maven_metadata_parse_failed':
        return `Failed to parse Forge maven-metadata.xml: ${e.details}`;
      case 'forge_installer_corrupted':
        return `Forge installer for ${e.mc}-${e.fv} is corrupted: ${e.details}`;
      case 'forge_unsupported_processor':
        return `This Forge version uses an unsupported processor: ${e.coord}`;
      case 'forge_patcher_failed':
        return `Forge patcher "${e.processor}" failed: ${e.details}`;
      case 'forge_mappings_missing':
        return `Forge mappings for ${e.mc} are not available`;
    }
  }

  async function refreshAccounts() {
    const list = await commands.listAccounts();
    if (list.status === 'ok') {
      accounts = list.data;
    } else {
      listAccountsError = errorMessage(list.error);
    }
    const active = await commands.getActiveAccount();
    if (active.status === 'ok') {
      activeAccount = active.data;
    }
  }

  onMount(async () => {
    void refreshInstances();

    events.processSpawned
      .listen((event) => {
        running = { pid: event.payload.pid, version_id: event.payload.version_id };
        exited = null;
      })
      .then((u) => {
        spawnUnlisten = u;
      });

    events.processExited
      .listen(async (event) => {
        running = null;
        exited = { code: event.payload.code, log_path: event.payload.log_path };
        void refreshInstances();
        if (event.payload.code !== 0 && activeInstance) {
          const result = await commands.latestCrash(activeInstance.id);
          if (result.status === 'ok' && result.data) {
            crashReport = result.data;
          }
        } else {
          crashReport = null;
        }
        await refreshViolations();
      })
      .then((u) => {
        exitUnlisten = u;
      });

    await refreshViolations();

    await refreshAccounts();

    const versionsResult = await commands.listVersions();
    if (versionsResult.status === 'ok') {
      versions = versionsResult.data;
    } else {
      versionsError = errorMessage(versionsResult.error);
    }
  });

  async function onAddOfflineSubmit() {
    const trimmed = offlineNameDraft.trim();
    if (trimmed.length === 0) {
      offlineNameError = 'Name cannot be empty';
      return;
    }
    offlineNameError = null;
    const result = await commands.addOfflineAccount(trimmed);
    if (result.status === 'ok') {
      await commands.setActiveAccount(result.data.id);
      await refreshAccounts();
      showAddOfflineInput = false;
      offlineNameDraft = '';
    } else {
      offlineNameError = errorMessage(result.error);
    }
  }

  async function onSelectAccount(id: string) {
    const result = await commands.setActiveAccount(id);
    if (result.status === 'ok') {
      await refreshAccounts();
    }
  }

  async function onRemoveActive() {
    if (!activeAccount) return;
    removeError = null;
    const result = await commands.removeAccount(activeAccount.id);
    if (result.status === 'ok') {
      await refreshAccounts();
    } else {
      removeError = errorMessage(result.error);
    }
  }

  function formatAccountLabel(a: Account): string {
    return `${a.name} (offline)`;
  }

  async function refreshInstances() {
    instancesError = null;
    const list = await commands.listInstances();
    if (list.status === 'ok') {
      instances = list.data;
    } else {
      instancesError = errorMessage(list.error);
      instances = [];
    }
    const active = await commands.getActiveInstance();
    if (active.status === 'ok') {
      activeInstance = active.data;
    } else {
      activeInstance = null;
    }
  }

  async function onSelectInstance(id: string) {
    const result = await commands.setActiveInstance(id);
    if (result.status === 'error') {
      instancesError = errorMessage(result.error);
      return;
    }
    await refreshInstances();
    // The $effect watching activeInstance.id clears per-instance error
    // banners (installError, modsError, exited, crashReport) automatically.
  }

  async function onPlay() {
    if (!activeInstance) return;
    if (activeInstance.mc_version === '') return;
    installing = true;
    installError = null;
    const result = await commands.installAndLaunch(activeInstance.id);
    installing = false;
    if (result.status === 'error') {
      installError = errorMessage(result.error);
    } else {
      await refreshInstances();
    }
  }

  async function onStop() {
    const result = await commands.stopMinecraft();
    if (result.status === 'error') {
      installError = errorMessage(result.error);
    }
  }

  function openCrashInLogs() {
    if (!crashReport) return;
    logsInitialPath = crashReport.path;
    logsOpen = true;
  }

  async function onOpenMods() {
    modsError = null;
    if (!activeInstance) return;
    const result = await commands.openModsFolder(activeInstance.id);
    if (result.status === 'error') {
      modsError = errorMessage(result.error);
    }
  }

  async function refreshViolations() {
    const v = await commands.networkAuditViolations();
    if (Array.isArray(v)) {
      violationsCount = v.length;
    }
  }
</script>

<main class="relative min-h-screen flex flex-col">
  <div class="flex-1 p-8 flex flex-col gap-6 items-start">
    <div class="absolute right-4 top-4 flex items-center gap-2">
      <button
        class="text-sm border rounded px-2 py-1 hover:bg-neutral-100"
        onclick={() => (logsOpen = !logsOpen)}
      >
        📜 Logs
      </button>
      <button
        class="text-sm border rounded px-2 py-1 hover:bg-neutral-100 relative"
        onclick={() => {
          networkOpen = !networkOpen;
          if (!networkOpen) void refreshViolations();
        }}
      >
        🌐 Network
        {#if violationsCount > 0}
          <span
            class="absolute -top-1 -right-1 inline-block w-2.5 h-2.5 bg-red-600 rounded-full"
            aria-label="{violationsCount} allowlist violations"
          ></span>
        {/if}
      </button>
      <NetworkPopover bind:open={networkOpen} />
      <LogsPopover
        bind:open={logsOpen}
        initialPath={logsInitialPath}
        instanceId={activeInstance?.id ?? null}
      />
    </div>

    <h1 class="text-2xl font-bold">FTlauncher</h1>

    {#if crashReport}
      <div
        class="w-full max-w-2xl bg-red-50 border border-red-300 text-red-800 px-3 py-2 rounded flex items-center justify-between gap-3"
      >
        <span class="text-sm">
          Minecraft crashed.
          <span class="font-mono text-xs">{crashReport.path.split(/[\\/]/).pop()}</span>
        </span>
        <div class="flex items-center gap-2">
          <button
            class="text-xs bg-red-600 text-white rounded px-2 py-1 hover:bg-red-700"
            onclick={openCrashInLogs}
          >
            View crash report
          </button>
          <button
            class="text-xs border border-red-400 rounded px-2 py-1 hover:bg-red-100"
            onclick={() => (crashReport = null)}
          >
            Dismiss
          </button>
        </div>
      </div>
    {/if}

    <section class="flex flex-col gap-2">
      <h2 class="text-sm font-semibold uppercase tracking-wide text-neutral-600">Account</h2>
      {#if accounts.length === 0}
        <p class="text-sm text-neutral-500">No accounts yet — add one below.</p>
      {:else}
        <div class="flex items-center gap-2">
          <label class="text-sm">Active:</label>
          <select
            class="border rounded px-2 py-1 w-64"
            value={activeAccount?.id ?? ''}
            onchange={(e) => onSelectAccount((e.currentTarget as HTMLSelectElement).value)}
          >
            {#each accounts as a}
              <option value={a.id}>{formatAccountLabel(a)}</option>
            {/each}
          </select>
          <button
            class="border rounded px-2 py-1 text-xs hover:bg-neutral-100"
            onclick={onRemoveActive}
            disabled={!activeAccount}
          >
            Remove
          </button>
        </div>
        {#if activeAccount}
          <p class="text-xs text-neutral-500 font-mono">UUID: {activeAccount.uuid}</p>
        {/if}
        {#if removeError}
          <p class="text-xs text-red-700">
            {removeError}
            <button
              class="text-neutral-500 hover:text-neutral-800"
              onclick={() => (removeError = null)}>×</button
            >
          </p>
        {/if}
      {/if}

      <div class="flex items-center gap-2 mt-2">
        <button
          class="border rounded px-3 py-1 text-sm hover:bg-neutral-100"
          onclick={() => (showAddOfflineInput = !showAddOfflineInput)}
        >
          + Add offline account
        </button>
      </div>
      <p class="text-xs text-neutral-500 italic">
        Microsoft account login is deferred — coming back after v0.5.0 (mod browser).
      </p>
      {#if listAccountsError}
        <p class="text-xs text-red-700">
          {listAccountsError}
          <button
            class="text-neutral-500 hover:text-neutral-800"
            onclick={() => (listAccountsError = null)}>×</button
          >
        </p>
      {/if}
      {#if showAddOfflineInput}
        <div class="flex items-center gap-2 mt-1">
          <input
            class="border rounded px-2 py-1 w-48 text-sm"
            placeholder="Player name"
            bind:value={offlineNameDraft}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                onAddOfflineSubmit();
              }
            }}
          />
          <button
            class="border rounded px-2 py-1 text-sm hover:bg-neutral-100"
            onclick={onAddOfflineSubmit}>Add</button
          >
          <button
            class="text-sm text-neutral-500 hover:text-neutral-800"
            onclick={() => {
              showAddOfflineInput = false;
              offlineNameDraft = '';
              offlineNameError = null;
            }}>Cancel</button
          >
        </div>
        {#if offlineNameError}
          <p class="text-xs text-red-700">{offlineNameError}</p>
        {/if}
      {/if}
    </section>

    <section class="flex flex-col gap-2">
      <h2 class="text-sm font-semibold uppercase tracking-wide text-neutral-600">Instance</h2>
      {#if instances.length === 0}
        <div
          class="text-sm text-neutral-700 bg-neutral-100 border border-neutral-300 rounded px-3 py-2"
        >
          No instances found — create one to get started.
          <button
            class="ml-2 bg-blue-600 text-white text-xs rounded px-2 py-1 hover:bg-blue-700"
            onclick={() => (manageOpen = true)}
          >
            + Create
          </button>
        </div>
      {:else}
        <div class="flex items-center gap-2">
          <select
            class="border rounded px-2 py-1 w-64"
            value={activeInstance?.id ?? ''}
            onchange={(e) => onSelectInstance((e.currentTarget as HTMLSelectElement).value)}
          >
            {#each instances as i}
              <option value={i.id}>
                {i.ready ? '✓' : '↓'}
                {i.name} · {i.loader}
                {i.mc_version || '(pick MC)'}
              </option>
            {/each}
          </select>
          <button
            class="border rounded px-2 py-1 text-xs hover:bg-neutral-100"
            onclick={() => (manageOpen = true)}
          >
            ⚙ Manage
          </button>
        </div>
        {#if instancesError}
          <p class="text-xs text-red-700">{instancesError}</p>
        {/if}
      {/if}
    </section>

    {#if activeInstance}
      <section class="flex flex-col gap-2">
        <h2 class="text-sm font-semibold uppercase tracking-wide text-neutral-600">
          Configuration
        </h2>
        <div class="text-sm grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1">
          <span class="text-neutral-500">Minecraft</span>
          <span class="font-mono">{activeInstance.mc_version || '(not set)'}</span>
          <span class="text-neutral-500">Loader</span>
          <span class="font-mono">
            {activeInstance.loader}{#if activeInstance.loader_version}
              · {activeInstance.loader_version}
            {/if}
          </span>
        </div>
        <p class="text-xs text-neutral-500">
          Edit version, loader, memory and JVM args via
          <button
            type="button"
            class="underline hover:text-neutral-800"
            onclick={() => (manageOpen = true)}>Manage</button
          >.
        </p>
      </section>

      <section class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          {#if running}
            <button
              class="bg-red-600 text-white px-3 py-1 rounded hover:bg-red-700"
              onclick={onStop}
            >
              ⏹ Stop
            </button>
            <span class="text-sm font-mono">
              Running {running.version_id} (PID {running.pid})
            </span>
          {:else if activeInstance.mc_version === ''}
            <button
              class="bg-neutral-300 text-neutral-600 px-3 py-1 rounded cursor-not-allowed"
              disabled
              title="Pick a Minecraft version first"
            >
              Play {activeInstance.name}
            </button>
          {:else if installing}
            <button class="bg-blue-400 text-white px-3 py-1 rounded cursor-not-allowed" disabled>
              Working…
            </button>
          {:else}
            <button
              class="bg-blue-600 text-white px-3 py-1 rounded hover:bg-blue-700"
              onclick={onPlay}
            >
              {activeInstance.ready ? 'Play' : 'Install'}
              {activeInstance.name}
            </button>
          {/if}
          {#if installError}
            <span class="text-xs text-red-700 flex items-center gap-1">
              {installError}
              <button
                class="text-neutral-500 hover:text-neutral-800"
                onclick={() => (installError = null)}
                aria-label="Dismiss"
              >
                ×
              </button>
            </span>
          {/if}
          {#if exited && !running}
            <span class="text-xs text-neutral-600">
              Exited (code {exited.code})
            </span>
          {/if}
        </div>
      </section>
    {/if}

    <section class="flex flex-col gap-2">
      <h2 class="text-sm font-semibold uppercase tracking-wide text-neutral-600">Mods</h2>
      <div class="flex items-center gap-3">
        <button class="border rounded px-3 py-1 text-sm hover:bg-neutral-100" onclick={onOpenMods}>
          📂 Open mods folder
        </button>
        {#if modsError}
          <span class="text-xs text-red-700 flex items-center gap-1">
            {modsError}
            <button
              class="text-neutral-500 hover:text-neutral-800"
              onclick={() => (modsError = null)}
              aria-label="Dismiss"
            >
              ×
            </button>
          </span>
        {/if}
      </div>
      <p class="text-xs text-neutral-500 italic">
        Vanilla Minecraft doesn't load mods — pick a loader (Fabric, Quilt or Forge) above first.
        NeoForge support arrives in v0.4.1.
      </p>
      <p class="text-xs text-neutral-500 italic">
        All launches currently share one Minecraft folder. Switching loader or MC version while mods
        are installed may surface Fabric cache quirks — per-profile isolation arrives in v0.3.0.
      </p>
    </section>
  </div>

  <PhaseStatusRow />

  <ManageInstancesModal
    bind:open={manageOpen}
    bind:instances
    bind:activeInstance
    {versions}
    onChanged={refreshInstances}
  />
</main>
