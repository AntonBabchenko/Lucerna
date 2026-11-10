<script lang="ts">
  // Confirm for "put the instance back on its modpack's platform": the
  // installed pack version is re-applied, which writes the pack's own
  // Minecraft / loader / loader version back into the instance. Opened from the
  // migration dialog's pack block, never from a version picker — so it speaks
  // of restoring, not of updating or switching.
  //
  // `diff` is the backend's authoritative answer for the archive actually
  // fetched: its `version_bump` is what will change. When it is null the
  // instance already runs the pack's platform, and there is nothing to confirm.
  import type { ModpackUpdateDiff } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ModpackDiffList from './ModpackDiffList.svelte';
  import SwitchRiskList from './SwitchRiskList.svelte';
  import { assessSwitchRisks, defaultBackupWorlds } from './switch-risks';

  let {
    diff,
    packName,
    busy,
    worldCount = 0,
    onCancel,
    onConfirm,
  }: {
    diff: ModpackUpdateDiff;
    packName: string;
    busy: boolean;
    /** The profile's worlds: `0` hides the backup choice, `null` = unknown
     *  (offered without a count) — as in the update and switch confirms. */
    worldCount?: number | null;
    onCancel: () => void;
    onConfirm: (opts: { backupWorlds: boolean }) => void;
  } = $props();

  // Re-applying the installed version: never a downgrade, and the user's own
  // mods are not in play — no pack file changes hands.
  const risks = $derived(
    assessSwitchRisks({
      direction: 'reinstall',
      versionBump: diff.version_bump,
      userAdded: 0,
      manual: 0,
      hasBundledFiles: false,
    }),
  );
  const changesFiles = $derived(diff.added.length + diff.updated.length + diff.removed.length > 0);

  // «Back up worlds first», the same choice the update and switch confirms
  // offer: a restore moves the worlds to another Minecraft version or loader,
  // so its default (a version bump) is on. `null` until the user touches it.
  let backupOverride = $state<boolean | null>(null);
  const backupWorlds = $derived(backupOverride ?? defaultBackupWorlds(diff));
</script>

{#snippet backupChoice()}
  {#if worldCount !== 0}
    <div class="flex flex-col gap-1">
      <label class="flex items-start gap-2 cursor-pointer text-sm text-primary">
        <input
          type="checkbox"
          class="mt-0.5"
          checked={backupWorlds}
          disabled={busy}
          aria-describedby="restore-backup-worlds-hint"
          onchange={(e) => (backupOverride = e.currentTarget.checked)}
          data-testid="restore-backup-worlds"
        />
        {worldCount === null
          ? $t('modpacks.update.backupWorldsNoCount')
          : $t('modpacks.update.backupWorlds', { count: worldCount })}
      </label>
      <p id="restore-backup-worlds-hint" class="text-xs text-secondary pl-6">
        {$t('modpacks.update.backupWorldsHint')}
      </p>
    </div>
  {/if}
{/snippet}

<Modal
  ariaLabelledby="pack-restore-title"
  onClose={onCancel}
  closeOnBackdrop={!busy}
  closeOnEscape={!busy}
  panelClass="w-[480px] max-h-[80vh] p-5 flex flex-col gap-3"
  dataTestid="pack-restore-dialog"
>
  {#if diff.version_bump}
    <h3 id="pack-restore-title" class="font-semibold text-base text-primary">
      <!-- A bump that keeps Minecraft only moves the loader: naming the
           Minecraft version the instance already runs would restore nothing. -->
      {diff.version_bump.old_game_version === diff.version_bump.new_game_version
        ? $t('modpacks.restore.titleLoader')
        : $t('modpacks.restore.title', { packMc: diff.version_bump.new_game_version })}
    </h3>
    <!-- Same-version re-apply normally changes no file, but the archive is
         diffed against the recorded pack state, which can have drifted — so
         the sentence follows the diff, never the expectation. -->
    {#if changesFiles}
      <SwitchRiskList {risks} />
      <p class="text-sm text-secondary">
        {$t('modpacks.restore.bodyWithChanges', { pack: packName })}
      </p>
      {@render backupChoice()}
      <ModpackDiffList {diff} />
    {:else}
      <p class="text-sm text-secondary">{$t('modpacks.restore.body', { pack: packName })}</p>
      <SwitchRiskList {risks} />
      {@render backupChoice()}
    {/if}
  {:else}
    <h3 id="pack-restore-title" class="font-semibold text-base text-primary">{packName}</h3>
    <p class="text-sm text-secondary" data-testid="pack-restore-already-matches">
      {$t('modpacks.restore.alreadyMatches')}
    </p>
  {/if}

  <div class="flex justify-end gap-2">
    <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={onCancel}>
      {$t('modpacks.restore.cancelBtn')}
    </button>
    {#if diff.version_bump}
      <BusyButton
        {busy}
        class="btn-warning btn-sm"
        data-testid="pack-restore-confirm"
        onclick={() => onConfirm({ backupWorlds: worldCount !== 0 && backupWorlds })}
      >
        {$t('modpacks.restore.confirmBtn')}
      </BusyButton>
    {/if}
  </div>
</Modal>
