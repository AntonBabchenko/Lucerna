<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type { LoaderKind, ModVersion, RepairPlan, VersionRef } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  // The diagnoser found the client carries enforced-channel mods the server
  // lacks (a "server side" reject). Each is installed; disabling it — reversible —
  // lets the user reconnect. Per-mod Disable mirrors the install card's per-mod
  // Install, and reuses execute_repair's existing DisableMod path (no parallel
  // backend). A `breaks` warning flags mods that depend on the one being disabled.
  let {
    plan,
    instanceId,
    mcVersion = null,
    loader = null,
    onClose,
  }: {
    plan: Extract<RepairPlan, { kind: 'disable_blocking_mods' }>;
    instanceId: string;
    mcVersion?: string | null;
    loader?: LoaderKind | null;
    onClose: () => void;
  } = $props();

  // sha1s whose disable is in flight, and those already disabled this session, so
  // the button can't be re-clicked (a second disable would error / re-toast).
  const disablingSha1s = new SvelteSet<string>();
  const disabledSha1s = new SvelteSet<string>();

  // The card's job is done once every listed mod has been disabled — switch the
  // footer to a clear "you're done, reconnect" signal.
  const allDisabled = $derived(
    plan.mods.length > 0 && plan.mods.every((m) => disabledSha1s.has(m.sha1)),
  );

  async function disableMod(sha1: string, name: string) {
    disablingSha1s.add(sha1);
    try {
      const res = await commands.executeRepair(instanceId, { kind: 'disable_mod', sha1 });
      if (res.status === 'ok') {
        disabledSha1s.add(sha1);
        pushSuccess($t('logs.repair.blockingMods.disabledToast', { name }));
      } else {
        pushWarning(
          $t('logs.repair.blockingMods.failedToast', { name, error: formatError(res.error) }),
        );
      }
    } finally {
      disablingSha1s.delete(sha1);
    }
  }

  // ── Replace-version path (the common version-mismatch fix) ────────────────
  // The reject log carries no target version, so the user reads it off the
  // disconnect screen's "Server has" column and picks it here. We list the mod's
  // versions by its installed project_id, then reuse execute_repair's existing
  // Reinstall (uninstall old + install target with required deps). Only mods with
  // a known source + a resolved MC/loader can be replaced; the rest keep Disable.
  type Blocking = (typeof plan.mods)[number];

  const versionsBySha1 = new SvelteMap<string, ModVersion[]>();
  const chosenVersionId = new SvelteMap<string, string>();
  const loadingVersions = new SvelteSet<string>();
  const replacingSha1s = new SvelteSet<string>();
  const replacedSha1s = new SvelteSet<string>();

  function canReplace(m: Blocking): boolean {
    return !!(m.source && m.project_id && mcVersion && loader);
  }

  async function loadVersions(m: Blocking) {
    if (!m.source || !m.project_id || !mcVersion || !loader) return;
    loadingVersions.add(m.sha1);
    try {
      const res = await commands.modsVersions(m.source, m.project_id, mcVersion, loader);
      if (res.status === 'ok') {
        versionsBySha1.set(m.sha1, res.data);
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
    const versionId = chosenVersionId.get(m.sha1);
    if (!m.source || !m.project_id || !versionId) return;
    const target: VersionRef = {
      source: m.source,
      project_id: m.project_id,
      version_id: versionId,
    };
    replacingSha1s.add(m.sha1);
    try {
      const res = await commands.executeRepair(instanceId, {
        kind: 'reinstall',
        old_sha1: m.sha1,
        target,
      });
      if (res.status === 'ok') {
        replacedSha1s.add(m.sha1);
        const label =
          versionsBySha1.get(m.sha1)?.find((v) => v.version_id === versionId)?.version_number ??
          versionId;
        pushSuccess(
          $t('logs.repair.blockingMods.replacedToast', { name: m.mod_id, version: label }),
        );
      } else {
        pushWarning(
          $t('logs.repair.blockingMods.replaceFailedToast', {
            name: m.mod_id,
            error: formatError(res.error),
          }),
        );
      }
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
        {#if replacedSha1s.has(m.sha1)}
          <p
            class="mt-1 text-xs font-semibold text-success"
            data-testid={`blocking-replaced-${m.sha1}`}
          >
            {$t('logs.repair.blockingMods.replacedReconnect')}
          </p>
        {:else if canReplace(m)}
          {#if !versionsBySha1.has(m.sha1)}
            <button
              type="button"
              class="btn-primary btn-xs mt-1 self-start"
              data-testid={`blocking-replace-${m.sha1}`}
              disabled={loadingVersions.has(m.sha1)}
              onclick={() => void loadVersions(m)}
            >
              {loadingVersions.has(m.sha1)
                ? $t('logs.repair.blockingMods.loadingVersions')
                : $t('logs.repair.blockingMods.replaceVersion')}
            </button>
          {:else if versionsBySha1.get(m.sha1)?.length === 0}
            <p class="mt-1 text-xs text-muted">
              {$t('logs.repair.blockingMods.noVersionsFound')}
            </p>
          {:else}
            <div class="mt-2 flex flex-col gap-2">
              <p class="text-xs text-muted">{$t('logs.repair.blockingMods.pickVersionLabel')}</p>
              <div data-testid={`blocking-version-select-${m.sha1}`}>
                <Select
                  value={chosenVersionId.get(m.sha1) ?? null}
                  options={(versionsBySha1.get(m.sha1) ?? []).map((v) => ({
                    value: v.version_id,
                    label: v.version_number,
                  }))}
                  onChange={(val) => chosenVersionId.set(m.sha1, String(val))}
                />
              </div>
              <button
                type="button"
                class="btn-primary btn-xs self-start"
                data-testid={`blocking-install-version-${m.sha1}`}
                disabled={!chosenVersionId.get(m.sha1) || replacingSha1s.has(m.sha1)}
                onclick={() => void replaceMod(m)}
              >
                {#if replacingSha1s.has(m.sha1)}
                  <span class="inline-flex items-center gap-1.5"
                    ><Spinner size="sm" label={$t('logs.repair.blockingMods.replacing')} />{$t(
                      'logs.repair.blockingMods.replacing',
                    )}</span
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
              disabled={disablingSha1s.has(m.sha1) || disabledSha1s.has(m.sha1)}
              onclick={() => void disableMod(m.sha1, m.mod_id)}
            >
              {#if disabledSha1s.has(m.sha1)}
                {$t('logs.repair.blockingMods.disabledLabel')}
              {:else if disablingSha1s.has(m.sha1)}
                <span class="inline-flex items-center gap-1.5"
                  ><Spinner size="sm" label={$t('logs.repair.working')} />{$t(
                    'logs.repair.working',
                  )}</span
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

  {#if allDisabled}
    <p class="mt-3 text-xs font-semibold text-success" data-testid="blocking-all-disabled">
      {$t('logs.repair.blockingMods.allDisabled')}
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
