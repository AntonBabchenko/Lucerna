<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type {
    LoaderKind,
    ModSource,
    ModVersion,
    RepairPlan,
    ResolvedCandidate,
    ResolvedMod,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { displayLoader } from '$lib/instances/loader-display';
  import { decideModInstall, type DepItem, type OptionalItem } from '$lib/mods/dep-prompt';
  import { buildInstalledDepLines } from '$lib/mods/install-summary';
  import type { UnresolvableDetail } from '$lib/mods/unresolvable-detail';
  import DependencyDialog from '$lib/mods/DependencyDialog.svelte';
  import { mcVersions } from '$lib/settings/state.svelte';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { Icon } from '$lib/ui/icons';

  // Server-side log diagnosis surfaced a set of mods the server requires that
  // the instance is missing. Each resolvable candidate gets a per-mod Install
  // button that runs the SAME flow as the mod browser
  // (decideModInstall → DependencyDialog → buildInstalledDepLines), so
  // dependencies are shown and the success toast is correct without a parallel
  // implementation. There is no bulk multi-select — the user installs each
  // confirmed/chosen mod individually, mirroring how the browser installs one
  // card at a time. Unresolved mods keep their existing "find it yourself"
  // affordance unchanged.

  let {
    plan,
    instanceId,
    mcVersion,
    loader,
    onClose,
  }: {
    plan: Extract<RepairPlan, { kind: 'install_missing_mods' }>;
    instanceId: string;
    // The active instance's MC version + loader — sourced from LogsPopover,
    // which reads them off the active instance. Both feed the SAME
    // decideModInstall ctx the mod browser builds.
    mcVersion: string;
    loader: LoaderKind;
    onClose: () => void;
  } = $props();

  const exact = $derived(plan.mods.filter((m) => m.tier.tier === 'exact'));
  const fuzzy = $derived(plan.mods.filter((m) => m.tier.tier === 'fuzzy'));
  const unresolved = $derived(plan.mods.filter((m) => m.tier.tier === 'unresolved'));

  // Which candidates' install flows are running, keyed by project_id. A Set so
  // installing candidate B while A is in flight doesn't clobber A's spinner —
  // same rationale as ModBrowseView's installingProjectIds.
  const installingProjectIds = new SvelteSet<string>();

  // Project_ids that finished installing this session. Their Install button
  // becomes a disabled "Installed" state so it can't be clicked again —
  // re-clicking would re-install and stack duplicate success toasts.
  const installedProjectIds = new SvelteSet<string>();

  // The dependency dialog state, populated when decideModInstall returns a
  // prompt. Identical shape to ModBrowseView's depPrompt — we reuse the same
  // DependencyDialog component and onConfirm flow verbatim.
  let depPrompt = $state<{
    primary: ModVersion;
    primaryProjectName: string;
    required: DepItem[];
    optional: OptionalItem[];
    incompatible: string[];
    unresolvable: UnresolvableDetail[];
    loaderRequirements: string[];
    loaderMismatch: { instanceLoader: string; modLoaders: LoaderKind[] } | null;
  } | null>(null);

  // A candidate is busy while its install runs OR while its dependency dialog
  // is open (the install flow's finally removes it from installingProjectIds
  // when it hands off to the dialog; onConfirm re-adds it). Mirrors
  // ModBrowseView.isCardBusy.
  function isCandidateBusy(projectId: string): boolean {
    return installingProjectIds.has(projectId) || depPrompt?.primary.project_id === projectId;
  }

  // Thin wrappers over the IPC commands — the SAME ones ModBrowseView injects
  // into decideModInstall.
  async function fetchProjectName(source: ModSource, id: string): Promise<string | null> {
    const proj = await commands.modsProject(source, id);
    return proj.status === 'ok' ? proj.data.summary.name : null;
  }
  async function fetchVersions(source: ModSource, id: string): Promise<ModVersion[] | null> {
    const res = await commands.modsVersions(source, id, null, null);
    return res.status === 'ok' ? res.data : null;
  }

  // Install a single resolved candidate. Mirrors ModBrowseView.startInstall:
  // resolve the pinned version → resolve plan → decide → install or prompt.
  async function installCandidate(candidate: ResolvedCandidate) {
    const projectId = candidate.target.project_id;
    installingProjectIds.add(projectId);
    try {
      // 1. Resolve the primary ModVersion. We pin to the exact version the
      //    resolver chose (candidate.target.version_id), so fetch the project's
      //    versions faceted by the instance's mc + loader and pick that id.
      const versions = await commands.modsVersions(
        candidate.target.source,
        candidate.target.project_id,
        mcVersion,
        loader,
      );
      if (versions.status === 'error') {
        pushWarning(
          $t('logs.repair.missingMods.failedToast', {
            name: candidate.display.name,
            error: formatError(versions.error),
          }),
        );
        return;
      }
      const primary =
        versions.data.find((v) => v.version_id === candidate.target.version_id) ?? versions.data[0];
      if (!primary) {
        pushWarning(
          $t('logs.repair.missingMods.failedToast', {
            name: candidate.display.name,
            error: $t('mods.browse.errorNoCompatibleVersion'),
          }),
        );
        return;
      }

      // 2. Resolve the install plan (deps, conflicts) — exactly as the browser.
      const plan = await commands.modsResolveInstallPlan(instanceId, primary, mcVersion, loader);
      if (plan.status === 'error') {
        pushWarning(
          $t('logs.repair.missingMods.failedToast', {
            name: candidate.display.name,
            error: formatError(plan.error),
          }),
        );
        return;
      }

      // 3. Decide: fast-path install or open the dependency dialog. Same ctx
      //    ModBrowseView builds.
      const decision = await decideModInstall(plan.data, primary, {
        loader,
        mcVersion,
        manifestOrder: mcVersions.value.map((v) => v.id),
        displayLoader,
        fetchProjectName,
        fetchVersions,
      });

      if (decision.kind === 'install') {
        const installed = await commands.modsInstallWithDeps(instanceId, candidate.target, []);
        if (installed.status === 'ok') {
          installedProjectIds.add(projectId);
          pushSuccess(
            $t('logs.repair.missingMods.installedToast', { name: decision.primaryProjectName }),
          );
        } else {
          pushWarning(
            $t('logs.repair.missingMods.failedToast', {
              name: decision.primaryProjectName,
              error: formatError(installed.error),
            }),
          );
        }
      } else {
        // Hand off to the dependency dialog; onConfirm finishes the install.
        depPrompt = decision.prompt;
      }
    } finally {
      installingProjectIds.delete(projectId);
    }
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

  {#snippet installButton(cand: ResolvedCandidate)}
    {@const pid = cand.target.project_id}
    {@const installed = installedProjectIds.has(pid)}
    {@const busy = isCandidateBusy(pid)}
    <span
      class="inline-flex ml-auto"
      use:tooltip={installed ? $t('common.installed') : $t('common.install')}
    >
      <button
        type="button"
        class={`btn-icon btn-icon-sm ${installed ? '!text-success' : '!text-accent'}`}
        data-testid={`missing-install-${pid}`}
        aria-label={installed ? $t('common.installed') : $t('common.install')}
        disabled={busy || installed}
        onclick={() => void installCandidate(cand)}
      >
        {#if installed}
          <Icon name="success" size={15} />
        {:else if busy}
          <Spinner size="sm" />
        {:else}
          <Icon name="download" size={15} />
        {/if}
      </button>
    </span>
  {/snippet}

  {#if exact.length > 0}
    <p class="mt-3 text-xs font-semibold">{$t('logs.repair.missingMods.exactHeading')}</p>
    {#each exact as rm (rm.cited.id)}
      {#if rm.tier.tier === 'exact'}
        {@const cand = rm.tier.candidate}
        <div class="mt-1 flex items-center gap-2 text-sm">
          <span>{cand.display.name}</span>
          <span class="text-xs text-muted">{cand.display.source} · {cand.version_label}</span>
          {#if label(rm)}<span class="text-xs text-muted">· {label(rm)}</span>{/if}
          {@render installButton(cand)}
        </div>
      {/if}
    {/each}
  {/if}

  {#if fuzzy.length > 0}
    <p class="mt-3 text-xs font-semibold">{$t('logs.repair.missingMods.fuzzyHeading')}</p>
    {#each fuzzy as rm (rm.cited.id)}
      {#if rm.tier.tier === 'fuzzy'}
        <p class="mt-2 text-xs text-muted">{rm.cited.id}</p>
        <div class="flex flex-col gap-1">
          {#each rm.tier.candidates as c (c.target.project_id)}
            <div class="flex items-center gap-2 text-sm">
              <span>{c.display.name}</span>
              <span class="text-xs text-muted">{c.display.source} · {c.version_label}</span>
              {@render installButton(c)}
            </div>
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
      class="btn-secondary btn-sm"
      data-testid="missing-cancel"
      onclick={onClose}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>

{#if depPrompt}
  <DependencyDialog
    primary={depPrompt.primary}
    primaryProjectName={depPrompt.primaryProjectName}
    required={depPrompt.required}
    optional={depPrompt.optional}
    incompatible={depPrompt.incompatible}
    unresolvable={depPrompt.unresolvable}
    loaderRequirements={depPrompt.loaderRequirements}
    onCancel={() => (depPrompt = null)}
    onConfirm={async (chosenOptional) => {
      const prompt = depPrompt;
      if (!prompt) {
        depPrompt = null;
        return;
      }
      depPrompt = null;
      // Keep the originating candidate busy while the confirmed install runs.
      installingProjectIds.add(prompt.primary.project_id);
      try {
        const installed = await commands.modsInstallWithDeps(
          instanceId,
          {
            source: prompt.primary.source,
            project_id: prompt.primary.project_id,
            version_id: prompt.primary.version_id,
          },
          chosenOptional.map((v) => ({
            source: v.source,
            project_id: v.project_id,
            version_id: v.version_id,
          })),
        );
        if (installed.status === 'ok') {
          installedProjectIds.add(prompt.primary.project_id);
          // Build the per-mod toast from the dialog's already-resolved project
          // names — same as ModBrowseView's onConfirm.
          const depLines = buildInstalledDepLines(prompt, chosenOptional);
          pushSuccess(
            $t('logs.repair.missingMods.installedToast', { name: prompt.primaryProjectName }),
            depLines,
          );
        } else {
          pushWarning(
            $t('logs.repair.missingMods.failedToast', {
              name: prompt.primaryProjectName,
              error: formatError(installed.error),
            }),
          );
        }
      } finally {
        installingProjectIds.delete(prompt.primary.project_id);
      }
    }}
  />
{/if}
