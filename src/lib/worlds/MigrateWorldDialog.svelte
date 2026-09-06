<script lang="ts">
  import {
    commands,
    events,
    type Error as IpcError,
    type InstanceWithStatus,
    type MigrationMode,
    type MigrationOutcome,
    type MigrationPlan,
    type World,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import { displayLoader } from '$lib/instances/loader-display';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { migrateWorld } from '$lib/tasks/adapters/world-migrate';
  import { taskFor } from '$lib/tasks/registry.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import DialogTitle from '$lib/ui/DialogTitle.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import {
    datapackSummary,
    migrateDisabledKey,
    splitTargets,
    verdictKey,
  } from '$lib/worlds/migrate-plan-text';

  // Copy or move one world into another instance (spec §7). Opened from
  // WorldDetailDialog's footer action; the size and backup count come from
  // the `World` row (A13), everything else from `world_migration_plan` for
  // the chosen target. The work itself runs through the task registry
  // (`migrateWorld`: lane serial, scoped to the SOURCE), so the corner strip
  // shows progress; this dialog stays open with its confirm busy until the
  // outcome arrives, then hands it to `onDone`. A running migration cannot be
  // cancelled (spec §4.2), so Cancel and the backdrop are locked while busy.

  let {
    instanceId,
    instanceName,
    world,
    instances,
    onClose,
    onDone,
  }: {
    instanceId: string;
    instanceName: string;
    world: World;
    instances: InstanceWithStatus[];
    onClose: () => void;
    onDone: (r: { mode: MigrationMode; outcome: MigrationOutcome; targetName: string }) => void;
  } = $props();

  let mode = $state<MigrationMode>('copy');
  let targetId = $state<string | null>(null);
  let plan = $state<MigrationPlan | null>(null);
  let planning = $state(false);
  let planError = $state<string | null>(null);
  let busy = $state(false);
  let actionError = $state<string | null>(null);
  // Instances whose game process is alive — their options are disabled with
  // the Quick Play "already running" reason (spec §7).
  let runningIds = $state<string[]>([]);

  const split = $derived(splitTargets(instances, instanceId));
  const excludedNames = $derived(split.excludedNoVersion.map((i) => i.name).join(', '));
  const selectedTarget = $derived(split.candidates.find((i) => i.id === targetId) ?? null);
  const options = $derived(
    split.candidates.map((i) => {
      const running = runningIds.includes(i.id);
      const base = `${i.name} · ${displayLoader(i.loader)} ${i.mc_version}`;
      return {
        value: i.id,
        label: running ? `${base} — ${$t('worlds.quickPlay.disabledRunning')}` : base,
        disabled: running,
      };
    }),
  );

  // Seed the running set once, then keep it live from the process events.
  // Fallback direction — PERMISSIVE, deliberately: this probe only decorates
  // the options; the authoritative gate is the backend's maintenance claim,
  // which refuses a running source or target with
  // `WorldMigrateInstanceRunning`, naming the instance and its role, and that
  // error renders inline below. Disabling every target on a failed advisory
  // probe would block a valid migration on a transient IPC failure and tell
  // the user nothing true about why.
  void (async () => {
    try {
      const rows = await commands.runningInstances();
      runningIds = rows.map((r) => r.instance_id);
    } catch {
      // Could not tell: leave the options enabled — see the direction note above.
    }
  })();
  listenUntilDestroyed([
    events.processSpawned.listen((e) => {
      const id = e.payload.instance_id;
      if (!runningIds.includes(id)) runningIds = [...runningIds, id];
    }),
    events.processExited.listen((e) => {
      runningIds = runningIds.filter((id) => id !== e.payload.instance_id);
    }),
  ]);

  // Monotonic guard: a slow plan for an earlier pick must not land over the
  // plan of the target now selected (the `createQuickWorlds` seq idiom).
  let planSeq = 0;
  async function loadPlan(toInstance: string) {
    const seq = ++planSeq;
    planning = true;
    plan = null;
    planError = null;
    const r = await commands.worldMigrationPlan(instanceId, world.folder_name, toInstance);
    if (seq !== planSeq) return;
    planning = false;
    if (r.status === 'ok') {
      plan = r.data;
    } else {
      // Inline, never a toast: the user is looking at this dialog, and a
      // `WorldInUse` here means exactly what a confirm would hit.
      planError = formatError(r.error);
    }
  }

  function pickTarget(v: string | number) {
    const id = String(v);
    targetId = id;
    actionError = null;
    void loadPlan(id);
  }

  const size = $derived(formatSize($t, world.size_bytes));
  const summary = $derived(plan === null ? null : datapackSummary(plan));
  const adopted = $derived(
    summary === null
      ? 0
      : summary.total - summary.inTarget - summary.keptNameTaken - summary.keptNotAdded,
  );
  const verdict = $derived.by(() => {
    if (plan === null || selectedTarget === null) return null;
    const key = verdictKey(plan.verdict);
    if (key === null) return null;
    return $t(key, {
      name: plan.world_version_name ?? $t('worlds.migrate.verdict.versionNameUnknown'),
      target: selectedTarget.mc_version,
    });
  });
  const verdictIsWarning = $derived.by(() => {
    if (plan === null) return false;
    return plan.verdict.kind === 'will_upgrade' || plan.verdict.kind === 'world_is_newer';
  });
  const disabledKey = $derived(
    migrateDisabledKey({
      fellBack: dataLocation.fellBack,
      hasCandidates: split.candidates.length > 0,
      sourceBusy: taskFor({ instanceId }) !== null,
      hasTarget: selectedTarget !== null,
      planning,
    }),
  );
  const disabledReason = $derived(disabledKey === null ? null : $t(disabledKey));

  // The adapter's error is `unknown`: a typed IPC error from `typedError`, or
  // a thrown `Error` (bridge failure). Only the former has translated copy.
  function describeError(e: unknown): string {
    if (e !== null && typeof e === 'object' && 'kind' in e && typeof e.kind === 'string') {
      return formatError(e as IpcError);
    }
    return e instanceof Error ? e.message : String(e);
  }

  async function onConfirm() {
    const target = selectedTarget;
    // Belt-and-braces: the button is disabled with a visible reason whenever
    // this would return; it only guards a programmatic click.
    if (target === null || disabledReason !== null) return;
    busy = true;
    actionError = null;
    try {
      const r = await migrateWorld(world.folder_name, {
        fromInstance: instanceId,
        worldFolder: world.folder_name,
        toInstance: target.id,
        mode,
      });
      if (r.status === 'ok') {
        onDone({ mode, outcome: r.outcome, targetName: target.name });
        return;
      }
      actionError =
        r.status === 'cancelled' ? $t('worlds.migrate.cancelledQueued') : describeError(r.error);
    } finally {
      busy = false;
    }
  }
</script>

<Modal
  ariaLabelledby="migrate-world-title"
  {onClose}
  dataTestid="migrate-world-dialog"
  panelClass="max-w-lg w-full p-4 flex flex-col gap-3"
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
>
  <DialogTitle id="migrate-world-title">
    {$t('worlds.migrate.title', { world: world.folder_name })}
  </DialogTitle>

  {#if split.candidates.length === 0}
    <p class="text-sm text-muted" data-testid="migrate-no-targets">
      {$t('worlds.migrate.noTargets')}
    </p>
  {:else}
    <div class="block">
      <span class="mb-1 block text-sm font-medium text-secondary">
        {$t('worlds.migrate.targetLabel')}
      </span>
      <Select
        class="w-full"
        value={targetId}
        {options}
        onChange={pickTarget}
        placeholder={$t('worlds.migrate.targetPlaceholder')}
        disabled={busy}
        ariaLabel={$t('worlds.migrate.targetLabel')}
        dataTestid="migrate-target"
      />
    </div>
  {/if}
  {#if split.excludedNoVersion.length > 0}
    <p class="text-xs text-muted" data-testid="migrate-excluded">
      {$t('worlds.migrate.excludedNoVersion', { names: excludedNames })}
    </p>
  {/if}

  <fieldset class="flex flex-col gap-2">
    <legend class="mb-1 text-sm font-medium text-secondary">
      {$t('worlds.migrate.modeLegend')}
    </legend>
    <label class="flex items-start gap-2">
      <input
        type="radio"
        name="migrate-mode"
        value="copy"
        bind:group={mode}
        disabled={busy}
        data-testid="migrate-mode-copy"
      />
      <span class="text-sm">
        <span class="font-medium">{$t('worlds.migrate.copyLabel')}</span>
        <br />
        <span class="text-secondary">
          {$t('worlds.migrate.copyDescription', { source: instanceName })}
        </span>
      </span>
    </label>
    <label class="flex items-start gap-2">
      <input
        type="radio"
        name="migrate-mode"
        value="move"
        bind:group={mode}
        disabled={busy}
        data-testid="migrate-mode-move"
      />
      <span class="text-sm">
        <span class="font-medium">{$t('worlds.migrate.moveLabel')}</span>
        <br />
        <span class="text-secondary">
          {$t('worlds.migrate.moveDescription', { source: instanceName })}
        </span>
      </span>
    </label>
  </fieldset>
  {#if mode === 'move'}
    <p
      class="flex items-start gap-2 rounded-md bg-warning-bg px-3 py-2 text-xs text-warning-text"
      data-testid="migrate-move-note"
    >
      <Icon name="warning" size={14} class="mt-0.5 shrink-0" />
      <span>{$t('worlds.migrate.moveShortcutsNote', { source: instanceName })}</span>
    </p>
  {/if}

  <div class="flex flex-col gap-1 text-sm" data-testid="migrate-summary">
    {#if size}
      <p class="text-secondary">{$t('worlds.migrate.size', { size })}</p>
    {/if}
    <p class="text-secondary" data-testid="migrate-backups">
      {#if mode === 'move'}
        {$t('worlds.migrate.backupsMove', { count: world.backup_count })}
      {:else}
        {$t('worlds.migrate.backupsCopy', { count: world.backup_count })}
      {/if}
    </p>
    {#if planning}
      <Spinner
        size="sm"
        labelPlacement="right"
        label={$t('worlds.migrate.planning')}
        class="text-secondary"
      />
    {:else if planError}
      <p class="text-xs text-danger" role="alert" data-testid="migrate-plan-error">{planError}</p>
    {:else if plan !== null && summary !== null}
      {#if plan.world_version_name}
        <p class="text-secondary">
          {$t('worlds.migrate.worldVersion', { name: plan.world_version_name })}
        </p>
      {/if}
      <p class="text-secondary" data-testid="migrate-datapacks">
        {$t('worlds.migrate.datapacks.total', { count: summary.total })}
      </p>
      {#if summary.inTarget > 0}
        <p class="pl-4 text-secondary">
          {$t('worlds.migrate.datapacks.inTarget', { count: summary.inTarget })}
        </p>
      {/if}
      {#if adopted > 0}
        <p class="pl-4 text-secondary">
          {$t('worlds.migrate.datapacks.adopted', { count: adopted })}
        </p>
      {/if}
      {#if summary.keptNameTaken > 0}
        <p class="pl-4 text-secondary">
          {$t('worlds.migrate.datapacks.keptNameTaken', { count: summary.keptNameTaken })}
        </p>
      {/if}
      {#if summary.keptNotAdded > 0}
        <p class="pl-4 text-secondary">
          {$t('worlds.migrate.datapacks.keptNotAdded', { count: summary.keptNotAdded })}
        </p>
      {/if}
      {#if summary.folders > 0}
        <p class="text-secondary">
          {$t('worlds.migrate.datapacks.folders', { count: summary.folders })}
        </p>
      {/if}
      {#if verdict !== null}
        <p
          class="flex items-start gap-2 {verdictIsWarning ? 'text-warning-text' : 'text-secondary'}"
          data-testid="migrate-verdict"
        >
          {#if verdictIsWarning}
            <Icon name="warning" size={14} class="mt-0.5 shrink-0" />
          {/if}
          <span>{verdict}</span>
        </p>
      {/if}
      {#if plan.mods_missing_in_target > 0}
        <p class="flex items-start gap-2 text-warning-text" data-testid="migrate-mods-missing">
          <Icon name="warning" size={14} class="mt-0.5 shrink-0" />
          <span>{$t('worlds.migrate.modsMissing', { count: plan.mods_missing_in_target })}</span>
        </p>
      {/if}
      {#if plan.source_loader !== plan.target_loader}
        <p class="text-secondary" data-testid="migrate-loader-note">
          {$t('worlds.migrate.loaderDiffers', {
            source: displayLoader(plan.source_loader),
            target: displayLoader(plan.target_loader),
          })}
        </p>
      {/if}
    {/if}
  </div>

  {#if actionError}
    <p class="text-xs text-danger" role="alert" data-testid="migrate-error">{actionError}</p>
  {/if}
  <div class="mt-1 flex items-center justify-end gap-2">
    {#if disabledReason !== null}
      <span class="mr-auto text-xs text-secondary" data-testid="migrate-disabled-reason">
        {disabledReason}
      </span>
    {/if}
    <button type="button" class="btn-secondary btn-sm" onclick={onClose} disabled={busy}>
      {$t('common.cancel')}
    </button>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <span
      class="inline-flex"
      tabindex={disabledReason !== null ? 0 : undefined}
      use:tooltip={{ text: disabledReason ?? '', describe: false }}
    >
      <BusyButton
        type="button"
        class="btn-primary btn-sm"
        {busy}
        disabled={disabledReason !== null}
        onclick={() => void onConfirm()}
        data-testid="migrate-confirm"
      >
        {mode === 'move' ? $t('worlds.migrate.moveBtn') : $t('worlds.migrate.copyBtn')}
      </BusyButton>
    </span>
  </div>
</Modal>
