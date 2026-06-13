<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type { RepairPlan, ResolvedMod, VersionRef } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  let {
    plan,
    instanceId,
    onClose,
  }: {
    plan: Extract<RepairPlan, { kind: 'install_missing_mods' }>;
    instanceId: string;
    onClose: () => void;
  } = $props();

  let selected = $state<Map<string, VersionRef>>(initialSelection());
  let busy = $state(false);

  function initialSelection(): Map<string, VersionRef> {
    const m = new Map<string, VersionRef>();
    for (const rm of plan.mods) {
      if (rm.tier.tier === 'exact') {
        m.set(rm.tier.candidate.target.project_id, rm.tier.candidate.target);
      }
    }
    return m;
  }

  /** Map from project_id to candidate display name for toast lookup. */
  const displayNameByProjectId = $derived(
    (() => {
      const map = new Map<string, string>();
      for (const rm of plan.mods) {
        if (rm.tier.tier === 'exact') {
          map.set(rm.tier.candidate.target.project_id, rm.tier.candidate.display.name);
        } else if (rm.tier.tier === 'fuzzy') {
          for (const c of rm.tier.candidates) {
            map.set(c.target.project_id, c.display.name);
          }
        }
      }
      return map;
    })(),
  );

  const exact = $derived(plan.mods.filter((m) => m.tier.tier === 'exact'));
  const fuzzy = $derived(plan.mods.filter((m) => m.tier.tier === 'fuzzy'));
  const unresolved = $derived(plan.mods.filter((m) => m.tier.tier === 'unresolved'));
  const count = $derived(selected.size);

  function toggleExact(target: VersionRef, on: boolean) {
    const next = new Map(selected);
    if (on) next.set(target.project_id, target);
    else next.delete(target.project_id);
    selected = next;
  }

  function pickFuzzy(citedId: string, target: VersionRef) {
    const next = new Map(selected);
    const row = fuzzy.find((m) => m.cited.id === citedId);
    if (row && row.tier.tier === 'fuzzy') {
      for (const c of row.tier.candidates) next.delete(c.target.project_id);
    }
    next.set(target.project_id, target);
    selected = next;
  }

  async function install() {
    busy = true;
    const targets = [...selected.values()];
    for (const target of targets) {
      const res = await commands.modsInstallWithDeps(instanceId, target, []);
      if (res.status === 'ok') {
        const name = displayNameByProjectId.get(target.project_id) ?? res.data.primary_name;
        const deps = res.data.installed_dependencies;
        if (deps.length > 0) {
          pushSuccess(
            $t('logs.repair.missingMods.installedToastWithDeps', {
              name,
              deps: deps.join(', '),
            }),
          );
        } else {
          pushSuccess($t('logs.repair.missingMods.installedToast', { name }));
        }
        const next = new Map(selected);
        next.delete(target.project_id);
        selected = next;
      } else {
        const name = displayNameByProjectId.get(target.project_id) ?? target.project_id;
        pushWarning(
          $t('logs.repair.missingMods.failedToast', {
            name,
            error: formatError(res.error),
          }),
        );
      }
    }
    busy = false;
    if (selected.size === 0) onClose();
  }

  function label(rm: ResolvedMod): string {
    return rm.cited.version
      ? $t('logs.repair.missingMods.serverWants', { version: rm.cited.version })
      : '';
  }
</script>

<div
  class="mt-3 rounded border border-warning-text/40 bg-surface p-3"
  data-testid="missing-mods-card"
>
  <p class="text-sm font-semibold">{$t('logs.repair.missingMods.title')}</p>
  <p class="mt-1 text-xs text-muted">{$t('logs.repair.missingMods.intro')}</p>

  {#if exact.length > 0}
    <p class="mt-3 text-xs font-semibold">{$t('logs.repair.missingMods.exactHeading')}</p>
    {#each exact as rm (rm.cited.id)}
      {#if rm.tier.tier === 'exact'}
        {@const cand = rm.tier.candidate}
        <label class="mt-1 flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            data-testid={`missing-exact-${cand.target.project_id}`}
            checked={selected.has(cand.target.project_id)}
            disabled={busy}
            onchange={(e) => toggleExact(cand.target, e.currentTarget.checked)}
          />
          <span>{cand.display.name}</span>
          <span class="text-xs text-muted">{cand.display.source} · {cand.version_label}</span>
          {#if label(rm)}<span class="text-xs text-muted">· {label(rm)}</span>{/if}
        </label>
        {#if cand.dependencies.length > 0}
          <span class="text-xs text-muted"
            >{$t('logs.repair.missingMods.dependsOn', {
              deps: cand.dependencies.join(', '),
            })}</span
          >
        {/if}
      {/if}
    {/each}
  {/if}

  {#if fuzzy.length > 0}
    <p class="mt-3 text-xs font-semibold">{$t('logs.repair.missingMods.fuzzyHeading')}</p>
    {#each fuzzy as rm (rm.cited.id)}
      {#if rm.tier.tier === 'fuzzy'}
        <p class="mt-2 text-xs text-muted">{rm.cited.id}</p>
        <div role="radiogroup" class="flex flex-col gap-1">
          {#each rm.tier.candidates as c (c.target.project_id)}
            <label class="flex items-center gap-2 text-sm">
              <input
                type="radio"
                name={`fuzzy-${rm.cited.id}`}
                data-testid={`missing-fuzzy-${c.target.project_id}`}
                checked={selected.get(c.target.project_id) === c.target}
                disabled={busy}
                onchange={() => pickFuzzy(rm.cited.id, c.target)}
              />
              <span>{c.display.name}</span>
              <span class="text-xs text-muted">{c.display.source} · {c.version_label}</span>
            </label>
            {#if c.dependencies.length > 0}
              <span class="text-xs text-muted"
                >{$t('logs.repair.missingMods.dependsOn', {
                  deps: c.dependencies.join(', '),
                })}</span
              >
            {/if}
          {/each}
        </div>
      {/if}
    {/each}
  {/if}

  {#if unresolved.length > 0}
    <p class="mt-3 text-xs font-semibold">{$t('logs.repair.missingMods.unresolvedHeading')}</p>
    {#each unresolved as rm (rm.cited.id)}
      <p class="text-sm" data-testid={`missing-unresolved-${rm.cited.id}`}>{rm.cited.id}</p>
    {/each}
  {/if}

  {#if plan.mods.length === unresolved.length}
    <p class="mt-2 text-xs text-muted">{$t('logs.repair.missingMods.none')}</p>
  {/if}

  <div class="mt-3 flex items-center gap-2">
    <button
      type="button"
      class="btn-primary btn-sm"
      data-testid="missing-install"
      disabled={count === 0 || busy}
      onclick={install}
    >
      {#if busy}
        <span class="inline-flex items-center gap-1.5"
          ><Spinner size="sm" label={$t('logs.repair.working')} />{$t('logs.repair.working')}</span
        >
      {:else}
        {$t('logs.repair.missingMods.install', { count })}
      {/if}
    </button>
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="missing-cancel"
      disabled={busy}
      onclick={onClose}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>
