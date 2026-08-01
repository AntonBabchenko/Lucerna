<script lang="ts">
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { commands, type WorldDatapack, type WorldPackState } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { pushInfo } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import CardShell from '$lib/ui/cards/CardShell.svelte';
  import CardMedia from '$lib/ui/cards/CardMedia.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
  import type { CardAccent, BadgeVariant } from '$lib/ui/cards/card-status';
  import { datapacksDisabledKey } from '$lib/worlds/datapacks-gating';

  // Per-world datapack manager. Library ∪ on-disk ∪ level.dat names, each row
  // carrying its own enabled/disabled/not_added/orphaned state — orphaned is
  // the repair path for Minecraft's "data packs are no longer present"
  // screen, so that row is the one deliberately rendered as a labelled
  // action rather than an icon, per the other rows' terser vocabulary.
  let {
    instanceId,
    world,
    running = false,
  }: { instanceId: string; world: string; running?: boolean } = $props();

  let packs = $state<WorldDatapack[]>([]);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let busy = $state(false);
  let busyAdd = $state(false);
  let busyRow = $state<string | null>(null);

  const disabledKey = $derived(datapacksDisabledKey({ running, busy }));
  const disabledReason = $derived.by(() => {
    const key = disabledKey;
    return key === null ? null : $t(key);
  });

  async function reload() {
    // Capture the world this load is for; a rapid world switch mid-fetch must
    // not commit the previous world's rows over the newer selection.
    const reqInstanceId = instanceId;
    const reqWorld = world;
    loadError = null;
    const res = await commands.datapacksListForWorld(reqInstanceId, reqWorld);
    if (instanceId !== reqInstanceId || world !== reqWorld) return;
    if (res.status === 'ok') {
      packs = res.data;
    } else {
      // Clear the list on failure too: an error and a stale, still-interactive
      // row list must never render together (see the template's mutually
      // exclusive loadError / empty / list branches below) — a row surviving
      // a failed reload would contradict whatever action just triggered it.
      packs = [];
      loadError = formatError(res.error);
    }
  }

  $effect(() => {
    void instanceId;
    void world;
    // A stale action error from the PREVIOUS world must not survive a world
    // switch and render on top of the new world's (perfectly valid) pack
    // list — same "stale message outlives its context" defect family as the
    // loadError fix above, just triggered by a prop change instead of a
    // failed reload.
    actionError = null;
    void reload();
  });

  function stateLabel(state: WorldPackState): string {
    switch (state) {
      case 'enabled':
        return $t('worlds.datapacks.stateEnabled');
      case 'disabled':
        return $t('worlds.datapacks.stateDisabled');
      case 'not_added':
        return $t('worlds.datapacks.stateNotAdded');
      case 'orphaned':
        return $t('worlds.datapacks.stateOrphaned');
    }
  }

  function stateBadgeVariant(state: WorldPackState): BadgeVariant {
    switch (state) {
      case 'enabled':
        return 'success';
      case 'disabled':
        return 'muted';
      case 'not_added':
        return 'neutral';
      case 'orphaned':
        return 'danger';
    }
  }

  // Orphaned (file gone) outranks a format mismatch for the row's accent —
  // there is nothing to read a pack_format from once the file is missing.
  function rowAccent(pack: WorldDatapack): CardAccent {
    if (pack.state === 'orphaned') return 'danger';
    if (pack.compat.kind === 'mismatch') return 'warning';
    return 'none';
  }

  async function addDatapackToLibrary() {
    actionError = null;
    const picked = await openFile({
      multiple: false,
      filters: [{ name: $t('common.fileFilter.datapack'), extensions: ['zip'] }],
    });
    if (typeof picked !== 'string') return;
    busyAdd = true;
    busy = true;
    try {
      const installed = await commands.datapacksInstallFromFile(instanceId, picked);
      if (installed.status !== 'ok') {
        actionError = formatError(installed.error);
        return;
      }
      const placed = await commands.datapacksAddToWorld(instanceId, world, installed.data.filename);
      if (placed.status === 'ok') {
        if (placed.data === 'copied') pushInfo($t('worlds.datapacks.copyNotLinked'));
        await reload();
      } else {
        actionError = formatError(placed.error);
      }
    } finally {
      busyAdd = false;
      busy = false;
    }
  }

  async function addToWorld(filename: string) {
    busyRow = filename;
    busy = true;
    actionError = null;
    try {
      const res = await commands.datapacksAddToWorld(instanceId, world, filename);
      if (res.status === 'ok') {
        if (res.data === 'copied') pushInfo($t('worlds.datapacks.copyNotLinked'));
        await reload();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRow = null;
      busy = false;
    }
  }

  async function removeFromWorld(filename: string) {
    busyRow = filename;
    busy = true;
    actionError = null;
    try {
      const res = await commands.datapacksRemoveFromWorld(instanceId, world, filename);
      if (res.status === 'ok') {
        await reload();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRow = null;
      busy = false;
    }
  }

  async function toggleEnabled(pack: WorldDatapack) {
    busyRow = pack.filename;
    busy = true;
    actionError = null;
    try {
      const res = await commands.datapacksSetEnabledInWorld(
        instanceId,
        world,
        pack.filename,
        pack.state !== 'enabled',
      );
      if (res.status === 'ok') {
        await reload();
      } else {
        actionError = formatError(res.error);
      }
    } finally {
      busyRow = null;
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-2" data-testid="world-datapacks">
  <div class="flex items-center justify-between gap-2">
    <h4 class="text-sm font-medium text-primary">{$t('worlds.datapacks.title')}</h4>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <span
      class="inline-flex"
      tabindex={disabledKey !== null ? 0 : undefined}
      use:tooltip={{ text: disabledReason ?? '', describe: false }}
    >
      <BusyButton
        class="btn-secondary btn-sm"
        busy={busyAdd}
        disabled={disabledKey !== null}
        onclick={() => void addDatapackToLibrary()}
        data-testid="world-datapack-add"
      >
        <Icon name="archive" size={14} />
        {$t('worlds.datapacks.add')}
      </BusyButton>
    </span>
  </div>

  {#if actionError}
    <p class="text-sm text-danger">{actionError}</p>
  {/if}

  {#if loadError}
    <p class="text-sm text-danger">{loadError}</p>
  {:else if packs.length === 0}
    <p class="text-sm text-muted">{$t('worlds.datapacks.empty')}</p>
  {:else}
    <div class="overflow-hidden rounded-lg border border-border-subtle">
      {#each packs as pack (pack.filename)}
        <CardShell variant="compact-row" accent={rowAccent(pack)}>
          <CardMedia placeholder="package" size="sm" />
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span
                class="font-mono text-xs truncate text-primary"
                use:tooltip={{ text: pack.filename, whenOverflowing: true }}
              >
                {pack.filename}
              </span>
              <StatusBadge
                variant={stateBadgeVariant(pack.state)}
                icon={pack.state === 'orphaned' ? 'circleX' : undefined}
              >
                {stateLabel(pack.state)}
              </StatusBadge>
              {#if !pack.in_library}
                <StatusBadge variant="neutral">{$t('worlds.datapacks.external')}</StatusBadge>
              {/if}
            </div>
            {#if pack.compat.kind === 'mismatch'}
              <p class="mt-0.5 text-xs text-warning-text">
                {$t('worlds.datapacks.formatMismatch', {
                  packFormat: pack.compat.pack_format,
                  expected: pack.compat.expected,
                })}
              </p>
            {/if}
            {#if pack.state === 'orphaned'}
              <p class="mt-0.5 text-xs text-danger">{$t('worlds.datapacks.orphanedHint')}</p>
            {/if}
          </div>
          <div class="flex flex-shrink-0 items-center gap-1">
            {#if pack.state === 'not_added'}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                use:tooltip={{
                  text: disabledReason ?? $t('worlds.datapacks.addToWorld'),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class="btn-icon btn-icon-sm !text-accent"
                  data-testid="world-datapack-add"
                  disabled={disabledKey !== null || busyRow === pack.filename}
                  aria-label={$t('worlds.datapacks.addToWorld')}
                  onclick={() => void addToWorld(pack.filename)}
                >
                  {#if busyRow === pack.filename}
                    <Spinner size="sm" />
                  {:else}
                    <Icon name="plus" size={15} />
                  {/if}
                </button>
              </span>
            {:else if pack.state === 'orphaned'}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null ? 0 : undefined}
                use:tooltip={{ text: disabledReason ?? '', describe: false }}
              >
                <BusyButton
                  class="btn-secondary btn-sm"
                  busy={busyRow === pack.filename}
                  disabled={disabledKey !== null}
                  onclick={() => void removeFromWorld(pack.filename)}
                  data-testid="world-datapack-remove"
                >
                  <Icon name="trash" size={14} />
                  {$t('worlds.datapacks.removeFromWorld')}
                </BusyButton>
              </span>
            {:else}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                use:tooltip={{
                  text:
                    disabledReason ??
                    (pack.state === 'enabled'
                      ? $t('worlds.datapacks.disable')
                      : $t('worlds.datapacks.enable')),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class={`btn-icon btn-icon-sm ${pack.state === 'enabled' ? 'btn-icon-success' : '!text-muted'}`}
                  data-testid="world-datapack-toggle"
                  disabled={disabledKey !== null || busyRow === pack.filename}
                  aria-label={pack.state === 'enabled'
                    ? $t('worlds.datapacks.disable')
                    : $t('worlds.datapacks.enable')}
                  onclick={() => void toggleEnabled(pack)}
                >
                  <Icon name="power" size={15} />
                </button>
              </span>
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <span
                class="inline-flex"
                tabindex={disabledKey !== null || busyRow === pack.filename ? 0 : undefined}
                use:tooltip={{
                  text: disabledReason ?? $t('worlds.datapacks.removeFromWorld'),
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class="btn-icon btn-icon-sm btn-icon-danger"
                  data-testid="world-datapack-remove"
                  disabled={disabledKey !== null || busyRow === pack.filename}
                  aria-label={$t('worlds.datapacks.removeFromWorld')}
                  onclick={() => void removeFromWorld(pack.filename)}
                >
                  <Icon name="trash" size={15} />
                </button>
              </span>
            {/if}
          </div>
        </CardShell>
      {/each}
    </div>
  {/if}
</div>
