<script lang="ts">
  // The one question every guarded mod operation asks (spec 2026-09-28 §6.1): the mods a
  // disable/removal would break, the disabled mods an enable needs, or a check that could not
  // run. Presentational: ModOpsHost feeds it the view from mod-ops and hands the choice back;
  // the mutation runs in mod-ops while `busy` names the spinning button.
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import type { ImpactChoice, ImpactView, OpsBusy } from '$lib/mods/mod-ops.svelte';

  let {
    view,
    busy = null,
    onChoose,
  }: {
    view: ImpactView;
    busy?: OpsBusy;
    onChoose: (choice: ImpactChoice) => void;
  } = $props();

  const count = $derived(view.targets.length);
  const name = $derived(view.targets[0]?.name ?? '');

  const title = $derived.by(() => {
    switch (view.mode) {
      case 'dependents-on-disable':
        return count === 1
          ? $t('mods.ops.disable.titleOne', { name })
          : $t('mods.ops.disable.titleMany', { count });
      case 'dependents-on-remove':
        return count === 1
          ? $t('mods.ops.remove.titleOne', { name })
          : $t('mods.ops.remove.titleMany', { count });
      case 'enable-with-requirements': {
        const deps = view.requirements.map((r) => r.name).join(', ');
        return count === 1
          ? $t('mods.ops.enable.titleOne', { name, deps })
          : $t('mods.ops.enable.titleMany', { count, deps });
      }
      case 'impact-check-failed':
        return view.action === 'enable'
          ? $t('mods.ops.checkFailed.requirements', { count })
          : $t('mods.ops.checkFailed.dependents', { count });
    }
  });

  const confirmLabel = $derived.by(() => {
    switch (view.mode) {
      case 'dependents-on-disable':
        return $t('mods.ops.disable.all', { count: count + view.dependents.length });
      case 'dependents-on-remove':
        return $t('mods.ops.remove.andDisable', { count: view.dependents.length });
      case 'enable-with-requirements':
        return $t('mods.ops.enable.together');
      case 'impact-check-failed':
        return view.action === 'disable'
          ? $t('mods.ops.checkFailed.disableAnyway')
          : view.action === 'uninstall'
            ? $t('mods.ops.checkFailed.removeAnyway')
            : $t('mods.ops.checkFailed.enableAnyway');
    }
  });

  // Removal is the one destructive confirm (spec §6.1); disabling and enabling are reversible.
  const removing = $derived(
    view.mode === 'dependents-on-remove' ||
      (view.mode === 'impact-check-failed' && view.action === 'uninstall'),
  );
  // A failed check has no "only this one": nothing is known to set it apart from.
  const secondaryLabel = $derived(
    view.mode === 'impact-check-failed' ? undefined : $t('mods.ops.onlyTargets', { count }),
  );
</script>

<ConfirmDialog
  {title}
  {confirmLabel}
  variant={removing ? 'danger' : 'primary'}
  busy={busy === 'primary'}
  secondaryBusy={busy === 'secondary'}
  {secondaryLabel}
  onSecondary={secondaryLabel === undefined ? undefined : () => onChoose('secondary')}
  confirmTestid="mod-impact-confirm"
  secondaryTestid="mod-impact-secondary"
  onCancel={() => onChoose('cancel')}
  onConfirm={() => onChoose('primary')}
>
  {#snippet body()}
    {#if view.mode === 'dependents-on-disable' || view.mode === 'dependents-on-remove'}
      <p class="text-sm text-secondary">{$t('mods.ops.dependents.lead', { count })}</p>
      <ul class="text-sm text-primary list-disc pl-5 max-h-40 overflow-auto selectable">
        {#each view.dependents as d (d.sha1)}
          <li>
            {d.name}{#if count > 1}<span class="text-muted">
                · {$t('mods.ops.dependents.needs', { names: d.needs.join(', ') })}</span
              >{/if}
          </li>
        {/each}
      </ul>
      <p class="text-sm text-secondary">{$t('mods.ops.dependents.note')}</p>
    {:else if view.mode === 'enable-with-requirements'}
      <p class="text-sm text-secondary">{$t('mods.ops.enable.body', { count })}</p>
    {:else}
      <p class="text-sm text-secondary selectable">{view.error}</p>
    {/if}
  {/snippet}
</ConfirmDialog>
