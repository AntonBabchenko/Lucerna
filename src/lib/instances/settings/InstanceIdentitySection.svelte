<script lang="ts">
  // The top of the Settings tab: the instance's picture, its name, its folder,
  // and the actions on the instance as a whole (clone, shortcut, translations,
  // open folder). Delete lives in the Danger zone at the bottom of the tab.
  import { onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { commands, type InstanceWithStatus } from '$lib/ipc/bindings';
  import InstanceAvatarEdit from '$lib/instances/InstanceAvatarEdit.svelte';
  import InstanceFolderRow from '$lib/instances/InstanceFolderRow.svelte';
  import { instanceIpcErrorMessage } from '$lib/instances/instance-ipc-error';
  import { shouldFocusField, type ManageFocusField } from '$lib/instances/manage-focus';
  import { t } from '$lib/i18n';
  import { fieldFlash } from '$lib/ui/field-flash';
  import { Icon } from '$lib/ui/icons';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { reportLateFailure } from './late-failure';

  let {
    instance,
    running,
    busy,
    focusField = null,
    onFocusDelivered = () => {},
    onChanged,
    onFolderRenamed = () => {},
    onClone,
    onShortcut,
    onTranslations,
    onOpenFolder,
  }: {
    instance: InstanceWithStatus;
    running: boolean;
    busy: boolean;
    focusField?: ManageFocusField | null;
    onFocusDelivered?: () => void;
    onChanged: () => void;
    /** The instance's folder was renamed, so its id is now `newId` — told
     *  before the refresh brings it in, so the tab knows it is the same one. */
    onFolderRenamed?: (newId: string) => void;
    onClone: (id: string) => void;
    /** Undefined where the OS has no desktop-shortcut support: the button is
     *  then omitted rather than shown disabled. */
    onShortcut?: (id: string) => void;
    onTranslations: (id: string) => void;
    onOpenFolder: (id: string) => void;
  } = $props();

  // Mirrors MAX_INSTANCE_NAME_LEN in src-tauri/src/commands/mod.rs — the backend
  // is the source of truth and rejects longer names; this only drives the input
  // maxlength + counter so the UI agrees with the validator.
  const NAME_MAX = 32;

  // See InstanceGameSection. Plain `let` (destroy-phase reads are stale).
  let unmounted = false;
  onDestroy(() => {
    unmounted = true;
  });
  function isStale(id: string): boolean {
    return unmounted || instance.id !== id;
  }

  let error = $state<string | null>(null);

  const SAVED_HINT_MS = 1500;
  let nameSaved = $state(false);
  let savedTimer: ReturnType<typeof setTimeout> | null = null;
  onDestroy(() => {
    if (savedTimer) clearTimeout(savedTimer);
  });
  function markNameSaved() {
    nameSaved = true;
    if (savedTimer) clearTimeout(savedTimer);
    savedTimer = setTimeout(() => (nameSaved = false), SAVED_HINT_MS);
  }

  // The editable name resyncs only when the INSTANCE changes, not on every
  // object-identity churn: a background refresh (game exit, integrity or import
  // completion) replaces the whole list with the same id, and gating on the id
  // keeps an in-progress edit from being silently clobbered. A folder rename
  // changes the id (the directory name IS the id) without being another
  // instance: `onRenamed` moves the cursor first, so the draft is kept.
  let nameDraft = $state('');
  let nameSyncId: string | null = null;
  $effect(() => {
    if (instance.id !== nameSyncId) {
      nameDraft = instance.name;
      nameSyncId = instance.id;
      nameSaved = false;
      error = null;
    }
  });

  async function commitName() {
    if (nameDraft === instance.name) return;
    if (!nameDraft.trim()) {
      // Empty/whitespace: snap back to the saved name rather than leaving a
      // blank box (an accidental clear reads as a silent no-op).
      nameDraft = instance.name;
      return;
    }
    const { id, name } = instance;
    if (!isStale(id)) error = null;
    const result = await commands.setInstanceName(id, nameDraft.trim());
    if (result.status === 'ok') {
      onChanged();
      if (!isStale(id)) markNameSaved();
      return;
    }
    const message = instanceIpcErrorMessage(result.error);
    if (isStale(id)) reportLateFailure(name, get(t)('instance.manage.nameLabel'), message);
    else error = message;
  }

  const folderLock = $derived(running || busy ? $t('instance.settings.folderLocked') : null);

  let nameInput = $state<HTMLInputElement | null>(null);
</script>

<section class="flex flex-col">
  <div class="mb-3 flex items-center gap-3">
    <InstanceAvatarEdit
      {instance}
      size={52}
      testId="settings-avatar"
      removeTestId="settings-avatar-remove"
    />
  </div>

  <div
    data-focus-field="name"
    use:fieldFlash={{
      active: focusField === 'name',
      focus: shouldFocusField('name'),
      // Rename lands here: the whole name is selected, ready to be typed over.
      onDelivered: () => {
        nameInput?.select();
        onFocusDelivered();
      },
    }}
  >
    <label for="detail-name" class="mb-1 flex justify-between text-xs text-secondary">
      <span>{$t('instance.manage.nameLabel')}</span>
      <span class="flex items-center gap-2">
        {#if nameSaved}
          <span class="inline-flex items-center gap-1 font-normal text-success">
            <Icon name="success" size={12} />
            {$t('instance.manage.saved')}
          </span>
        {/if}
        <span class="text-placeholder font-normal"
          >{$t('instance.manage.nameCounter', { count: nameDraft.length, max: NAME_MAX })}</span
        >
      </span>
    </label>
    <input
      id="detail-name"
      bind:this={nameInput}
      class="border rounded px-2 py-1 w-full mb-3"
      maxlength={NAME_MAX}
      bind:value={nameDraft}
      oninput={() => (nameSaved = false)}
      onblur={commitName}
    />
  </div>
  <StatusMessage tone="danger" message={error} class="mb-2" />

  <InstanceFolderRow
    {instance}
    formatIpcError={instanceIpcErrorMessage}
    disabledReason={folderLock}
    onRenamed={(updated) => {
      // The directory name IS the id, so a rename changes it. The tab follows:
      // the cursor moves first so the name draft is not reseeded, and the
      // refresh brings the renamed instance in as the active one (the backend
      // re-points it).
      nameSyncId = updated.id;
      onFolderRenamed(updated.id);
      onChanged();
    }}
  />

  <div class="mb-3 flex flex-wrap gap-2" data-tour-ctx="settings-actions">
    <span
      class="inline-flex"
      use:tooltip={{ text: running ? $t('instance.manage.runningBlocked') : '', describe: false }}
    >
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1.5"
        disabled={running}
        onclick={() => onClone(instance.id)}
        data-testid="clone-instance-btn"
      >
        <Icon name="copy" size={14} />
        {$t('instance.manage.cloneBtn')}
      </button>
    </span>
    {#if onShortcut}
      <button
        type="button"
        class="btn-secondary btn-sm inline-flex items-center gap-1.5"
        onclick={() => onShortcut?.(instance.id)}
        data-testid="create-shortcut-btn"
      >
        <Icon name="monitor" size={14} />
        {$t('shortcut.create')}
      </button>
    {/if}
    <button
      type="button"
      class="btn-secondary btn-sm inline-flex items-center gap-1.5"
      onclick={() => onTranslations(instance.id)}
      data-testid="manage-translations-btn"
    >
      <Icon name="languages" size={14} />
      {$t('instance.manage.translationsBtn')}
    </button>
    <button
      type="button"
      class="btn-secondary btn-sm inline-flex items-center gap-1.5"
      onclick={() => onOpenFolder(instance.id)}
      data-testid="settings-open-folder-btn"
    >
      <Icon name="folderOpen" size={14} />
      {$t('instance.manage.openFolderBtn')}
    </button>
  </div>
</section>
