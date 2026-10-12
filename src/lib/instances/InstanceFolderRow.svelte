<script lang="ts">
  // The instance's on-disk folder name, and the warning when that name stops
  // the game from launching.
  //
  // Lives in its own component so the Settings tab's identity section stays
  // small.
  //
  // Why the folder name is surfaced at all: it can diverge from the display name
  // (renaming the instance never renamed its directory), and on Windows an
  // instance whose folder holds characters the system ANSI code page cannot
  // express cannot be launched — the JVM receives the path with `?` substituted.
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type { InstanceWithStatus, Error as IpcError, PathStatus } from '$lib/ipc/bindings';
  import RenameFolderDialog from './RenameFolderDialog.svelte';

  let {
    instance,
    formatIpcError,
    onRenamed,
    disabledReason = null,
  }: {
    instance: InstanceWithStatus;
    formatIpcError: (e: IpcError) => string;
    onRenamed: (updated: InstanceWithStatus) => void;
    /** Why the folder cannot be renamed right now (the game runs, a task runs on
     *  the instance) — the backend would refuse; shown under the row, and the
     *  rename buttons are off. Null = renamable. */
    disabledReason?: string | null;
  } = $props();

  let pathStatus = $state<PathStatus>('ok');
  let dialogSeed = $state<string | null>(null);

  // Re-checked per selected instance: the answer depends on this machine's code
  // page, so it cannot be derived from the instance record alone.
  $effect(() => {
    const id = instance.id;
    // Cancellation flag rather than re-reading `instance` after the await: the
    // tab can unmount (or the instance change) while the call is in flight.
    let cancelled = false;
    void (async () => {
      const result = await commands.instancePathStatus(id);
      if (cancelled) return;
      pathStatus = result.status === 'ok' ? result.data : 'ok';
    })();
    return () => {
      cancelled = true;
    };
  });

  async function openFixDialog() {
    // Seed with what the display name WOULD produce, so the common case is one
    // click and confirm.
    const suggestion = await commands.previewInstanceDirName(instance.name);
    dialogSeed = suggestion || instance.id;
  }
</script>

{#if pathStatus === 'instance_dir'}
  <div class="rounded border border-danger/40 bg-danger/10 p-3 mb-3 flex flex-col gap-2">
    <p class="text-sm font-medium text-primary">{$t('instance.manage.folderBrokenTitle')}</p>
    <p class="text-xs text-secondary">{$t('instance.manage.folderBrokenBody')}</p>
    <div>
      <button
        type="button"
        class="btn-primary btn-sm"
        disabled={disabledReason !== null}
        onclick={() => void openFixDialog()}
      >
        {$t('instance.manage.folderBrokenFix')}
      </button>
    </div>
  </div>
{:else if pathStatus === 'data_root'}
  <!-- Renaming the instance cannot help here: the unreadable characters are in
       the data-root path itself (e.g. the Windows user name under %APPDATA%).
       Deliberately NO rename button — offering one would send the user in a
       circle. The copy names the remedy (move the data folder); wiring a jump
       into the data-root setting from here is a follow-up. -->
  <div class="rounded border border-danger/40 bg-danger/10 p-3 mb-3 flex flex-col gap-2">
    <p class="text-sm font-medium text-primary">{$t('instance.manage.dataRootBrokenTitle')}</p>
    <p class="text-xs text-secondary">{$t('instance.manage.dataRootBrokenBody')}</p>
  </div>
{/if}

<div class="mb-3 flex flex-col gap-1">
  <span class="text-xs text-secondary">{$t('instance.manage.folderLabel')}</span>
  <div class="flex items-center gap-2 min-w-0">
    <code class="text-xs text-primary truncate flex-1 min-w-0">{instance.id}</code>
    <button
      type="button"
      class="btn-secondary btn-sm shrink-0"
      disabled={disabledReason !== null}
      aria-describedby={disabledReason !== null ? 'instance-folder-locked' : undefined}
      onclick={() => (dialogSeed = instance.id)}
    >
      {$t('instance.manage.folderChange')}
    </button>
  </div>
  {#if disabledReason !== null}
    <p id="instance-folder-locked" class="text-xs text-secondary">{disabledReason}</p>
  {/if}
  <p class="text-xs text-muted">{$t('instance.manage.folderHint')}</p>
</div>

{#if dialogSeed !== null}
  <RenameFolderDialog
    instanceId={instance.id}
    initialValue={dialogSeed}
    {formatIpcError}
    onCancel={() => (dialogSeed = null)}
    onRenamed={(updated) => {
      dialogSeed = null;
      onRenamed(updated);
    }}
  />
{/if}
