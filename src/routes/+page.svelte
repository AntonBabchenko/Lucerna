<script lang="ts">
  import {
    commands,
    events,
    type Account,
    type CrashReport,
    type Error as IpcError,
    type LoaderVersion,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import NetworkPopover from '$lib/network/NetworkPopover.svelte';
  import PhaseStatusRow from '$lib/install/PhaseStatusRow.svelte';
  import LogsPopover from '$lib/logs/LogsPopover.svelte';
  import { onMount } from 'svelte';

  let account = $state<Account | null>(null);
  let nameDraft = $state('');
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let networkOpen = $state(false);

  let versions = $state<VersionEntry[]>([]);
  let versionsLoading = $state(true);
  let versionsError = $state<string | null>(null);
  let showSnapshots = $state(false);
  let selectedId = $state<string | null>(null);

  type LoaderKind = 'vanilla' | 'fabric' | 'quilt';
  let loaderKind = $state<LoaderKind>('vanilla');
  let loaderVersions = $state<LoaderVersion[]>([]);
  let loaderVersion = $state<string | null>(null);
  let loaderLoading = $state(false);
  let loaderError = $state<string | null>(null);
  let loaderMetaOffline = $state(false);

  let effectiveVersionId = $derived(
    loaderKind === 'vanilla' || !selectedId || !loaderVersion
      ? selectedId
      : `${loaderKind}-loader-${loaderVersion}-${selectedId}`,
  );

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

  let visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );

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
      case 'io':
        return `IO error at ${e.path}: ${e.details}`;
    }
  }

  onMount(async () => {
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
        if (event.payload.code !== 0) {
          const result = await commands.latestCrash();
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

    const accountResult = await commands.getAccount();
    if (accountResult.status === 'ok') {
      account = accountResult.data;
      nameDraft = accountResult.data?.name ?? '';
    } else {
      saveError = errorMessage(accountResult.error);
    }

    const versionsResult = await commands.listVersions();
    if (versionsResult.status === 'ok') {
      versions = versionsResult.data;
      const firstRelease = versions.find((v) => v.version_type === 'release');
      selectedId = firstRelease?.id ?? versions[0]?.id ?? null;
    } else {
      versionsError = errorMessage(versionsResult.error);
    }
    versionsLoading = false;
  });

  $effect(() => {
    // Reset + refetch whenever the MC version OR loader kind changes.
    if (loaderKind === 'vanilla' || !selectedId) {
      loaderVersions = [];
      loaderVersion = null;
      loaderError = null;
      loaderLoading = false;
      loaderMetaOffline = false;
      return;
    }
    const mc = selectedId;
    const kind = loaderKind;
    loaderLoading = true;
    loaderError = null;
    loaderMetaOffline = false;
    (async () => {
      const result =
        kind === 'fabric'
          ? await commands.listFabricLoaders(mc)
          : await commands.listQuiltLoaders(mc);
      // Discard the result if the user changed selection while waiting.
      if (selectedId !== mc || loaderKind !== kind) {
        return;
      }
      if (result.status === 'ok') {
        loaderVersions = result.data;
        const firstStable = result.data.find((v) => v.stable);
        loaderVersion = (firstStable ?? result.data[0])?.version ?? null;
      } else {
        loaderVersions = [];
        loaderVersion = null;
        loaderError = errorMessage(result.error);
      }
      loaderLoading = false;
    })();
  });

  async function saveName() {
    const trimmed = nameDraft.trim();
    if (trimmed.length === 0) return;
    if (trimmed === account?.name) return;
    saving = true;
    saveError = null;
    const result = await commands.setOfflineAccount(trimmed);
    if (result.status === 'ok') {
      account = result.data;
    } else {
      saveError = errorMessage(result.error);
    }
    saving = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      (e.currentTarget as HTMLInputElement).blur();
    }
  }

  function formatVersionLabel(v: VersionEntry): string {
    const type =
      v.version_type === 'release'
        ? 'release'
        : v.version_type === 'snapshot'
          ? 'snapshot'
          : v.version_type === 'old_beta'
            ? 'beta'
            : 'alpha';
    return `${v.id} (${type})`;
  }

  async function onPlay() {
    if (!effectiveVersionId) return;
    installing = true;
    installError = null;
    const result = await commands.installAndLaunch(effectiveVersionId);
    installing = false;
    if (result.status === 'error') {
      installError = errorMessage(result.error);
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
    const result = await commands.openModsFolder();
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
      <LogsPopover bind:open={logsOpen} initialPath={logsInitialPath} />
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
      <label class="flex flex-col gap-1">
        <span class="text-sm">Your name</span>
        <input
          class="border rounded px-2 py-1 w-64"
          bind:value={nameDraft}
          onblur={saveName}
          onkeydown={onKey}
          placeholder="Type a name and press Enter"
          disabled={saving}
        />
      </label>
      {#if account}
        <p class="text-xs text-neutral-500 font-mono">UUID: {account.uuid}</p>
      {/if}
      {#if saveError}
        <p class="text-xs text-red-700 flex items-center gap-2">
          Could not save: {saveError}
          <button
            class="text-neutral-500 hover:text-neutral-800"
            onclick={() => (saveError = null)}
            aria-label="Dismiss"
          >
            ×
          </button>
        </p>
      {/if}
    </section>

    <section class="flex flex-col gap-2">
      <h2 class="text-sm font-semibold uppercase tracking-wide text-neutral-600">Version</h2>
      {#if versionsLoading}
        <p class="text-sm text-neutral-500">Loading versions…</p>
      {:else if versionsError}
        <p class="text-sm text-red-700">Could not load versions: {versionsError}</p>
      {:else}
        <div class="flex items-center gap-3">
          <select class="border rounded px-2 py-1 w-64" bind:value={selectedId}>
            {#each visibleVersions as v}
              <option value={v.id}>{formatVersionLabel(v)}</option>
            {/each}
          </select>
          <label class="text-sm flex items-center gap-1">
            <input type="checkbox" bind:checked={showSnapshots} />
            Show snapshots
          </label>
        </div>
        <p class="text-xs text-neutral-500">
          {visibleVersions.length} version{visibleVersions.length === 1 ? '' : 's'} available
        </p>
        <div class="flex items-center gap-3 mt-2">
          <label class="text-sm flex items-center gap-2">
            Loader:
            <select class="border rounded px-2 py-1 w-32" bind:value={loaderKind}>
              <option value="vanilla">Vanilla</option>
              <option value="fabric">Fabric</option>
              <option value="quilt">Quilt</option>
            </select>
          </label>
          {#if loaderKind !== 'vanilla'}
            <label class="text-sm flex items-center gap-2">
              Version:
              {#if loaderLoading}
                <span class="text-xs text-neutral-500 italic">Loading…</span>
              {:else if loaderError}
                <span class="text-xs text-red-700">{loaderError}</span>
              {:else}
                <select
                  class="border rounded px-2 py-1 w-48"
                  bind:value={loaderVersion}
                  disabled={loaderVersions.length === 0}
                >
                  {#each loaderVersions as lv}
                    <option value={lv.version}>
                      {lv.version}{lv.stable ? '' : ' (beta)'}
                    </option>
                  {/each}
                </select>
              {/if}
              {#if loaderMetaOffline}
                <span class="text-xs text-neutral-500 italic">(offline — using cached list)</span>
              {/if}
            </label>
          {/if}
        </div>
        <div class="flex items-center gap-3">
          {#if running}
            <button
              class="bg-red-600 text-white px-3 py-1 rounded hover:bg-red-700"
              onclick={onStop}
            >
              Stop
            </button>
            <span class="text-sm font-mono">
              Running {running.version_id} (PID {running.pid})
            </span>
          {:else}
            <button
              class="bg-green-600 text-white px-3 py-1 rounded hover:bg-green-700 disabled:opacity-50"
              disabled={!effectiveVersionId ||
                installing ||
                !nameDraft.trim() ||
                loaderLoading ||
                (loaderKind !== 'vanilla' && !loaderVersion)}
              onclick={onPlay}
            >
              {installing ? 'Working…' : `Play ${effectiveVersionId ?? ''}`}
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
      {/if}
    </section>

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
        Vanilla Minecraft doesn't load mods — pick a loader (Fabric or Quilt) above first. Forge / NeoForge support arrives in v0.4.0.
      </p>
      <p class="text-xs text-neutral-500 italic">
        All launches currently share one Minecraft folder. Switching loader or MC version while mods are installed may surface Fabric cache quirks — per-profile isolation arrives in v0.3.0.
      </p>
    </section>
  </div>

  <PhaseStatusRow />
</main>
