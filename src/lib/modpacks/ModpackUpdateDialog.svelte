<script lang="ts">
  import type { ModpackUpdateDiff } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import ModpackDiffList from './ModpackDiffList.svelte';
  import { defaultBackupWorlds } from './switch-risks';

  let {
    diff,
    worldCount = 0,
    onCancel,
    onConfirm,
  }: {
    diff: ModpackUpdateDiff;
    /** The profile's worlds: `0` hides the backup choice, `null` = unknown
     *  (offered without a count). */
    worldCount?: number | null;
    onCancel: () => void;
    onConfirm: (opts: { backupWorlds: boolean }) => void;
  } = $props();

  // `null` until the user touches the box: the default follows the update's risk.
  let override = $state<boolean | null>(null);
  const backupWorlds = $derived(override ?? defaultBackupWorlds(diff));
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

  {#if worldCount !== 0}
    <div class="flex flex-col gap-1">
      <label class="flex items-start gap-2 cursor-pointer text-sm text-primary">
        <input
          type="checkbox"
          class="mt-0.5"
          checked={backupWorlds}
          aria-describedby="update-backup-worlds-hint"
          onchange={(e) => (override = e.currentTarget.checked)}
          data-testid="update-backup-worlds"
        />
        {worldCount === null
          ? $t('modpacks.update.backupWorldsNoCount')
          : $t('modpacks.update.backupWorlds', { count: worldCount })}
      </label>
      <p id="update-backup-worlds-hint" class="text-xs text-secondary pl-6">
        {$t('modpacks.update.backupWorldsHint')}
      </p>
    </div>
  {/if}

  <ModpackDiffList {diff} />

  <div class="flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" onclick={onCancel}
      >{$t('modpacks.update.cancelBtn')}</button
    >
    <button
      type="button"
      class="btn-warning btn-sm"
      onclick={() => onConfirm({ backupWorlds: worldCount !== 0 && backupWorlds })}
      data-testid="update-confirm"
    >
      {$t('modpacks.update.updateBtn')}
    </button>
  </div>
</Modal>
