<script lang="ts">
  // One-click Optimise preview. Shows the resolved curated performance set for
  // the instance: what will install, what is already there, and what is not
  // applicable (no build for this MC / OptiFine conflict). Confirm installs the
  // `will_install` entries through the normal dependency-aware pipeline.
  import type { OptimisePlan, OptimiseEntry, LoaderKind } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { displayLoader } from '$lib/instances/loader-display';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';

  let {
    plan,
    loader,
    mc,
    installing = false,
    onConfirm,
    onCancel,
  }: {
    plan: OptimisePlan;
    loader: LoaderKind;
    mc: string;
    installing?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();

  const willInstall = $derived(plan.entries.filter((e) => e.status.status === 'will_install'));
  const alreadyInstalled = $derived(
    plan.entries.filter((e) => e.status.status === 'already_installed'),
  );
  // Everything shown but not installable: no build / optifine conflict / unknown.
  const skipped = $derived(
    plan.entries.filter((e) =>
      ['unavailable_for_version', 'conflict_optifine', 'unknown'].includes(e.status.status),
    ),
  );

  function skippedReason(e: OptimiseEntry): string {
    switch (e.status.status) {
      case 'unavailable_for_version':
        return $t('optimise.statusUnavailable', { mc });
      case 'conflict_optifine':
        return $t('optimise.statusConflictOptifine');
      default:
        return $t('optimise.statusUnknown');
    }
  }
</script>

<!-- While installing, block implicit close paths — the in-flight IPC per mod
     cannot be aborted mid-run. -->
<Modal
  ariaLabel={$t('optimise.dialogTitle')}
  onClose={onCancel}
  closeOnBackdrop={!installing}
  closeOnEscape={!installing}
>
  <header class="p-4 border-b">
    <h2 class="text-base font-semibold">{$t('optimise.dialogTitle')}</h2>
    <p class="text-sm text-muted mt-1">
      {$t('optimise.intro', { loader: displayLoader(loader) })}
    </p>
  </header>

  <div class="p-4 flex flex-col gap-4 max-h-[55vh] overflow-y-auto">
    {#if willInstall.length > 0}
      <section class="flex flex-col gap-1">
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('optimise.sectionInstall')}
        </div>
        {#each willInstall as e (e.key)}
          <div class="flex items-baseline justify-between text-sm">
            <span class="text-success">
              {e.title}
              <span class="text-muted font-mono">{e.version_number}</span>
            </span>
            {#if e.note === 'single_player_tick'}
              <span class="text-xs text-muted">{$t('optimise.noteSinglePlayerTick')}</span>
            {/if}
          </div>
        {/each}
      </section>
    {/if}

    {#if alreadyInstalled.length > 0}
      <section class="flex flex-col gap-1">
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('optimise.sectionInstalled')}
        </div>
        {#each alreadyInstalled as e (e.key)}
          <div class="text-sm text-muted">{e.title}</div>
        {/each}
      </section>
    {/if}

    {#if skipped.length > 0}
      <section class="flex flex-col gap-1">
        <div class="text-[10px] uppercase tracking-wider text-muted">
          {$t('optimise.sectionSkipped')}
        </div>
        {#each skipped as e (e.key)}
          <div class="flex items-baseline justify-between text-sm text-muted">
            <span>{e.title}</span>
            <span class="text-xs">{skippedReason(e)}</span>
          </div>
        {/each}
      </section>
    {/if}

    {#if plan.install_count === 0}
      <p class="text-sm text-muted">{$t('optimise.nothingToInstall')}</p>
    {/if}
  </div>

  <footer class="p-4 border-t flex justify-end gap-2">
    <button type="button" class="btn-ghost btn-sm" disabled={installing} onclick={onCancel}>
      {$t('optimise.cancel')}
    </button>
    <BusyButton
      busy={installing}
      disabled={plan.install_count === 0}
      class="btn-primary btn-sm"
      onclick={onConfirm}
    >
      {$t('optimise.install', { count: plan.install_count })}
    </BusyButton>
  </footer>
</Modal>
