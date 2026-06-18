<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type { LoaderKind, RepairPlan, VersionRef } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { deferOrRunRepair, isCompleted, isDeferred } from '$lib/logs/deferred-repairs.svelte';
  import {
    getChosenVersion,
    getLoadedVersions,
    setChosenVersion,
    setLoadedVersions,
  } from '$lib/logs/blocking-replace-state.svelte';

  // A "server side" reject means the client carries enforced-channel mods at a
  // version the server doesn't accept. The common fix is to REPLACE the mod with
  // the version the server wants (read off the disconnect screen's "Server has"
  // column); disabling is the fallback when the server lacks the mod entirely.
  // Both actions reuse execute_repair via the deferred-repairs store, which runs
  // them now or — if a game is running — queues them for after it closes.
  let {
    plan,
    instanceId,
    mcVersion = null,
    loader = null,
    gameRunning = false,
    onClose,
  }: {
    plan: Extract<RepairPlan, { kind: 'disable_blocking_mods' }>;
    instanceId: string;
    mcVersion?: string | null;
    loader?: LoaderKind | null;
    gameRunning?: boolean;
    onClose: () => void;
  } = $props();

  type Blocking = (typeof plan.mods)[number];

  // In-flight markers (component-local). The chosen version, loaded version list,
  // and completion/queued state live in module stores so they survive the card
  // unmounting when the Logs window closes.
  const disablingSha1s = new SvelteSet<string>();
  const replacingSha1s = new SvelteSet<string>();
  const loadingVersions = new SvelteSet<string>();

  // A row is "settled" once its repair has been applied (completed) or queued for
  // after the game closes. The footer flips to done when every mod is settled.
  function isSettled(m: Blocking): boolean {
    return isCompleted(instanceId, m.sha1) || isDeferred(instanceId, m.sha1);
  }
  const allResolved = $derived(plan.mods.length > 0 && plan.mods.every((m) => isSettled(m)));

  function canReplace(m: Blocking): boolean {
    return !!(m.source && m.project_id && mcVersion && loader);
  }

  async function disableMod(m: Blocking) {
    disablingSha1s.add(m.sha1);
    try {
      await deferOrRunRepair(gameRunning, {
        instanceId,
        sha1: m.sha1,
        label: m.mod_id,
        choice: { kind: 'disable_mod', sha1: m.sha1 },
      });
    } finally {
      disablingSha1s.delete(m.sha1);
    }
  }

  async function loadVersions(m: Blocking) {
    if (!m.source || !m.project_id || !mcVersion || !loader) return;
    loadingVersions.add(m.sha1);
    try {
      const res = await commands.modsVersions(m.source, m.project_id, mcVersion, loader);
      if (res.status === 'ok') {
        setLoadedVersions(instanceId, m.sha1, res.data);
      } else {
        pushWarning(
          $t('logs.repair.blockingMods.replaceFailedToast', {
            name: m.mod_id,
            error: formatError(res.error),
          }),
        );
      }
    } finally {
      loadingVersions.delete(m.sha1);
    }
  }

  async function replaceMod(m: Blocking) {
    const versionId = getChosenVersion(instanceId, m.sha1);
    if (!m.source || !m.project_id || !versionId) return;
    const target: VersionRef = {
      source: m.source,
      project_id: m.project_id,
      version_id: versionId,
    };
    replacingSha1s.add(m.sha1);
    try {
      await deferOrRunRepair(gameRunning, {
        instanceId,
        sha1: m.sha1,
        label: m.mod_id,
        choice: { kind: 'reinstall', old_sha1: m.sha1, target },
      });
    } finally {
      replacingSha1s.delete(m.sha1);
    }
  }
</script>

<div
  class="mt-3 rounded border border-warning-text/40 bg-surface p-3"
  data-testid="blocking-mods-card"
>
  <p class="text-sm font-semibold">{$t('logs.repair.blockingMods.title')}</p>
  <p class="mt-1 text-xs text-muted" data-testid="blocking-intro">
    {$t('logs.repair.blockingMods.intro')}
  </p>

  <div class="mt-3 flex flex-col gap-2">
    {#each plan.mods as m (m.sha1)}
      <div class="text-sm">
        <span class="font-mono">{m.mod_id}</span>
        {#if isCompleted(instanceId, m.sha1)}
          <p
            class="mt-1 text-xs font-semibold text-success"
            data-testid={`blocking-replaced-${m.sha1}`}
          >
            {$t('logs.repair.blockingMods.replacedReconnect')}
          </p>
        {:else if isDeferred(instanceId, m.sha1)}
          <p
            class="mt-1 text-xs font-semibold text-accent"
            data-testid={`blocking-queued-${m.sha1}`}
          >
            {$t('logs.repair.blockingMods.queued')}
          </p>
        {:else if canReplace(m)}
          {#if getLoadedVersions(instanceId, m.sha1) === null}
            <button
              type="button"
              class="btn-primary btn-xs mt-1 self-start"
              data-testid={`blocking-replace-${m.sha1}`}
              disabled={loadingVersions.has(m.sha1)}
              onclick={() => void loadVersions(m)}
            >
              {#if loadingVersions.has(m.sha1)}
                <span class="inline-flex items-center gap-1.5">
                  <Spinner size="sm" />{$t('logs.repair.blockingMods.loadingVersions')}
                </span>
              {:else}
                {$t('logs.repair.blockingMods.replaceVersion')}
              {/if}
            </button>
          {:else if getLoadedVersions(instanceId, m.sha1)?.length === 0}
            <p class="mt-1 text-xs text-muted">
              {$t('logs.repair.blockingMods.noVersionsFound')}
            </p>
          {:else}
            <div class="mt-2 flex flex-col gap-2">
              <p class="text-xs text-muted">{$t('logs.repair.blockingMods.pickVersionLabel')}</p>
              <div data-testid={`blocking-version-select-${m.sha1}`}>
                <Select
                  value={getChosenVersion(instanceId, m.sha1)}
                  options={(getLoadedVersions(instanceId, m.sha1) ?? []).map((v) => ({
                    value: v.version_id,
                    label: v.version_number,
                  }))}
                  onChange={(val) => setChosenVersion(instanceId, m.sha1, String(val))}
                />
              </div>
              <button
                type="button"
                class="btn-primary btn-xs self-start"
                data-testid={`blocking-install-version-${m.sha1}`}
                disabled={!getChosenVersion(instanceId, m.sha1) || replacingSha1s.has(m.sha1)}
                onclick={() => void replaceMod(m)}
              >
                {#if replacingSha1s.has(m.sha1)}
                  <span class="inline-flex items-center gap-1.5"
                    ><Spinner size="sm" />{$t('logs.repair.blockingMods.replacing')}</span
                  >
                {:else}
                  {$t('logs.repair.blockingMods.installThisVersion')}
                {/if}
              </button>
            </div>
          {/if}
        {/if}
        <details class="mt-1" data-testid={`blocking-disclosure-${m.sha1}`}>
          <summary class="cursor-pointer text-xs text-secondary hover:text-primary">
            {$t('logs.repair.blockingMods.disableQuestion')}
          </summary>
          <div class="mt-2 flex flex-col gap-2">
            <p class="text-xs text-muted">{$t('logs.repair.blockingMods.disableHint')}</p>
            {#if m.breaks.length > 0}
              <p class="text-xs text-warning-text/80" data-testid={`blocking-breaks-${m.sha1}`}>
                {$t('logs.repair.blockingMods.breaks', { names: m.breaks.join(', ') })}
              </p>
            {/if}
            <button
              type="button"
              class="btn-warning btn-xs self-start"
              data-testid={`blocking-disable-${m.sha1}`}
              aria-label={`${$t('logs.repair.blockingMods.disable')} ${m.mod_id}`}
              disabled={disablingSha1s.has(m.sha1) || isSettled(m)}
              onclick={() => void disableMod(m)}
            >
              {#if isCompleted(instanceId, m.sha1)}
                {$t('logs.repair.blockingMods.disabledLabel')}
              {:else if disablingSha1s.has(m.sha1)}
                <span class="inline-flex items-center gap-1.5"
                  ><Spinner size="sm" />{$t('logs.repair.working')}</span
                >
              {:else}
                {$t('logs.repair.blockingMods.disable')}
              {/if}
            </button>
          </div>
        </details>
      </div>
    {/each}
  </div>

  {#if allResolved}
    <p class="mt-3 text-xs font-semibold text-success" data-testid="blocking-all-disabled">
      {$t('logs.repair.blockingMods.allResolved')}
    </p>
  {:else}
    <p class="mt-3 text-xs text-muted">{$t('logs.repair.blockingMods.reconnectHint')}</p>
  {/if}

  <div class="mt-3 flex items-center gap-2">
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="blocking-cancel"
      onclick={onClose}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>
