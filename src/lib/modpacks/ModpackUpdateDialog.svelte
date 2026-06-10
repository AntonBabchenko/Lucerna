<script lang="ts">
  import type { ModpackUpdateDiff } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';

  let {
    diff,
    onCancel,
    onConfirm,
  }: {
    diff: ModpackUpdateDiff;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<Modal
  ariaLabelledby="modpack-update-title"
  onClose={onCancel}
  panelClass="w-[480px] max-h-[80vh] p-5 flex flex-col gap-3"
>
  <h3 id="modpack-update-title" class="font-semibold text-base text-primary">
    {$t('modpacks.update.title', { version: diff.new_version_number })}
  </h3>

  <div class="text-sm text-secondary">
    {$t('modpacks.update.changeSummary', {
      added: diff.added.length,
      removed: diff.removed.length,
      updated: diff.updated.length,
    })}
  </div>

  {#if diff.version_bump}
    <div class="text-sm bg-warning-bg border border-warning-text/30 rounded p-2 text-warning-text">
      {$t('modpacks.update.versionBump', {
        oldVersion: diff.version_bump.old_game_version,
        newVersion: diff.version_bump.new_game_version,
      })}
    </div>
  {/if}

  <div
    class="flex-1 overflow-y-auto border rounded divide-y text-sm"
    data-testid="update-diff-list"
  >
    {#each diff.added as f (f.install_path)}
      <div class="px-2 py-1 text-success flex items-center gap-1.5">
        <Icon name="plus" />
        {f.name}
      </div>
    {/each}
    {#each diff.updated as e (e.new.install_path)}
      <div class="px-2 py-1 text-accent flex items-center gap-1.5">
        <Icon name="update" />
        {e.new.name}
      </div>
    {/each}
    {#each diff.removed as f (f.install_path)}
      <div class="px-2 py-1 text-danger line-through flex items-center gap-1.5">
        <Icon name="minus" />
        {f.name}
      </div>
    {/each}
    {#if diff.added.length + diff.updated.length + diff.removed.length === 0}
      <div class="px-2 py-3 text-muted text-center">{$t('modpacks.update.noChanges')}</div>
    {/if}
  </div>

  <div class="flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}
      >{$t('modpacks.update.cancelBtn')}</button
    >
    <button
      type="button"
      class="btn-primary btn-sm"
      onclick={onConfirm}
      data-testid="update-confirm"
    >
      {$t('modpacks.update.updateBtn')}
    </button>
  </div>
</Modal>
