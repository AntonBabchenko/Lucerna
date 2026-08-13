<script lang="ts">
  import { commands, type OrphanedBackupSet, type StrandedWorld } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { t, locale } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import BackupsPanel from '$lib/worlds/BackupsPanel.svelte';

  // The two artefacts a failed restore can leave behind, and the only place in
  // the app that shows either. Both live under names every other listing filters
  // out (a leading dot fails `validate_segment`), so without this section the
  // data is reachable only through the OS file manager.
  //
  // Rendered by WorldsTab AFTER its exclusive {#if} chain, deliberately: a
  // section placed inside the chain's {:else} would never appear on an instance
  // whose only world was the one that got stranded — the exact case it exists
  // for.

  let {
    instanceId,
    orphans,
    stranded,
    onChanged,
  }: {
    instanceId: string;
    orphans: OrphanedBackupSet[];
    stranded: StrandedWorld[];
    onChanged: () => void;
  } = $props();

  let openFor = $state<string | null>(null);
  let recovering = $state<string | null>(null);
  let error = $state<string | null>(null);

  // A `.tmp-restoring-*` directory does NOT mean the restore failed. The success
  // path's cleanup is best-effort, so one survives a restore that finished — and
  // then `saves/<world>` holds the RESTORED world while the parked directory
  // holds the pre-restore one. Telling that user their restore "didn't finish",
  // and offering to put the old copy back over the new one, would be actively
  // harmful: the refusal they would hit reads "rename or remove it first".
  const unfinished = $derived(stranded.filter((s) => !s.target_occupied));
  const leftovers = $derived(stranded.filter((s) => s.target_occupied));

  async function openSaves() {
    const r = await commands.openSavesFolder(instanceId);
    if (r.status !== 'ok') error = formatError(r.error);
  }

  async function recover(s: StrandedWorld) {
    recovering = s.dir_name;
    error = null;
    const r = await commands.recoverStrandedWorld(instanceId, s.dir_name);
    recovering = null;
    if (r.status === 'ok') {
      pushSuccess($t('worlds.stranded.toastRecovered', { world: r.data }));
      onChanged();
    } else {
      error = formatError(r.error);
    }
  }
</script>

{#if error}
  <p class="text-sm text-danger mt-4" role="alert">{error}</p>
{/if}

{#if unfinished.length > 0}
  <section class="mt-6" aria-labelledby="worlds-stranded-title" data-testid="stranded-section">
    <h3 id="worlds-stranded-title" class="text-sm font-semibold text-primary mb-1">
      {$t('worlds.stranded.title')}
    </h3>
    <ul class="border border-border-subtle rounded divide-y divide-border-subtle">
      {#each unfinished as s (s.dir_name)}
        <li class="flex items-center justify-between gap-3 px-3 py-2">
          <p class="text-sm text-secondary min-w-0">
            {$t('worlds.stranded.description', { worldFolder: s.world_folder })}
          </p>
          <BusyButton
            type="button"
            class="btn-primary btn-sm flex-shrink-0"
            data-testid="stranded-recover-btn"
            busy={recovering === s.dir_name}
            onclick={() => void recover(s)}
          >
            {$t('worlds.stranded.recoverBtn')}
          </BusyButton>
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if leftovers.length > 0}
  <section class="mt-6" aria-labelledby="worlds-leftover-title" data-testid="leftover-section">
    <h3 id="worlds-leftover-title" class="text-sm font-semibold text-primary mb-1">
      {$t('worlds.stranded.leftoverTitle')}
    </h3>
    <ul class="border border-border-subtle rounded divide-y divide-border-subtle">
      {#each leftovers as s (s.dir_name)}
        <li class="flex items-center justify-between gap-3 px-3 py-2">
          <p class="text-sm text-secondary min-w-0">
            {$t('worlds.stranded.leftoverDescription', { worldFolder: s.world_folder })}
          </p>
          <!-- No "put it back": the world it would overwrite is the one the user
               asked for. Deleting is theirs to do, in their own file manager. -->
          <button
            type="button"
            class="btn-secondary btn-sm inline-flex items-center gap-1 flex-shrink-0"
            data-testid="leftover-open-folder-btn"
            onclick={() => void openSaves()}
          >
            <Icon name="folderOpen" size={14} />
            {$t('worlds.stranded.openFolderBtn')}
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if orphans.length > 0}
  <section class="mt-6" aria-labelledby="worlds-orphaned-title" data-testid="orphaned-section">
    <h3 id="worlds-orphaned-title" class="text-sm font-semibold text-primary mb-1">
      {$t('worlds.orphaned.title')}
    </h3>
    <p class="text-sm text-muted mb-2">{$t('worlds.orphaned.description')}</p>
    <ul class="border border-border-subtle rounded divide-y divide-border-subtle">
      {#each orphans as o (o.world_folder)}
        <li class="flex items-center justify-between gap-3 px-3 py-2">
          <div class="min-w-0">
            <div class="text-sm font-medium truncate">{o.world_folder}</div>
            <div class="text-xs text-muted">
              {$t('worlds.tab.backupCountAriaLabel', { count: o.backup_count })}
              {#if o.newest_unix_ms}
                · {new Date(o.newest_unix_ms).toLocaleDateString($locale ?? undefined)}
              {/if}
            </div>
          </div>
          <button
            type="button"
            class="btn-secondary btn-sm flex-shrink-0"
            data-testid="orphaned-open-btn"
            onclick={() => (openFor = o.world_folder)}
          >
            {$t('worlds.orphaned.openBtn')}
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

{#if openFor}
  <Modal
    ariaLabelledby="orphaned-backups-title"
    onClose={() => (openFor = null)}
    panelClass="max-w-lg w-full p-4"
  >
    <h3 id="orphaned-backups-title" class="font-semibold text-lg text-primary mb-3">
      {openFor}
    </h3>
    <!-- The panel takes a folder NAME, which is exactly why an orphaned set can
         reuse it unchanged. Locked to as_copy: `replace` calls `world_dir_at`
         and would fail WorldNotFound for a world that no longer exists. -->
    <BackupsPanel
      {instanceId}
      worldFolder={openFor}
      lockedMode="as_copy"
      canBackup={false}
      modeNote={$t('worlds.orphaned.asCopyNote')}
      onClose={() => (openFor = null)}
      onChanged={() => {
        onChanged();
      }}
    />
  </Modal>
{/if}
