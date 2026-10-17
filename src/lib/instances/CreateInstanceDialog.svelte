<script lang="ts">
  // Create an instance. Was the create form of the Manage window; it is a dialog
  // of its own now that instance settings are a tab, opened by New instance in
  // the sidebar, its empty state, and the Settings tab's own empty state.
  import { get } from 'svelte/store';
  import {
    commands,
    type LoaderKind,
    type MemoryBounds,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import { formatHeapLabel } from '$lib/instances/heap';
  import { instanceIpcErrorMessage } from '$lib/instances/instance-ipc-error';
  import { displayLoader } from '$lib/instances/loader-display';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { FALLBACK_MEMORY_BOUNDS, loadMemoryBounds } from '$lib/instances/memory-bounds';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import { t } from '$lib/i18n';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import VersionListNotice from '$lib/versions/VersionListNotice.svelte';
  import { LOADED_VERSION_LIST, type VersionListState } from '$lib/versions/mc-versions.svelte';

  let {
    versions,
    versionList = LOADED_VERSION_LIST,
    activate,
    onCreated,
    onClose,
  }: {
    versions: VersionEntry[];
    /** Loading / failed state of `versions`, said under the picker when it shows none. */
    versionList?: VersionListState;
    /** Make the new instance active; resolves to an error message or null. The
     *  page's own activation, so the same request bookkeeping applies. */
    activate: (id: string) => Promise<string | null>;
    onCreated: (id: string) => void;
    onClose: () => void;
  } = $props();

  // Mirrors MAX_INSTANCE_NAME_LEN in src-tauri/src/commands/mod.rs — the backend
  // is the source of truth and rejects longer names; this only drives the input
  // maxlength + counter so the UI agrees with the validator.
  const NAME_MAX = 32;

  let draftName = $state('');
  let draftMc = $state('');
  let draftLoader = $state<LoaderKind>('vanilla');
  let draftLoaderVersion = $state<string | null>(null);
  // Guards a double-clicked Create from spawning two instances (the reason stays
  // satisfied across the await, so only a busy flag stops re-entry).
  let createPending = $state(false);
  let error = $state<string | null>(null);

  // Snapshot toggle for the version picker. Off by default — most users want
  // stable releases.
  let showSnapshots = $state(false);
  const visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );
  // Keep the empty "Choose…" row so Select greys the trigger until one is picked.
  const mcVersionOptions = $derived([
    { value: '', label: $t('instance.manage.chooseMcOption') },
    ...visibleVersions.map((v) => ({ value: v.id, label: v.id })),
  ]);

  // `null` = untouched, so the effective value tracks the adaptive default until
  // the user actually drags — which also sidesteps the race where the bounds
  // resolve only after the dialog is already open.
  let createHeapDraft = $state<number | null>(null);
  let createBounds = $state<MemoryBounds>(FALLBACK_MEMORY_BOUNDS);
  $effect(() => {
    let alive = true;
    void loadMemoryBounds().then((b) => {
      if (alive) createBounds = b;
    });
    return () => {
      alive = false;
    };
  });
  // Guard the IPC boundary: a malformed payload must not seed NaN into the
  // slider (which would render an empty box and submit a NaN heap).
  const createHeapMb = $derived(
    createHeapDraft ??
      (Number.isFinite(createBounds.default_mb)
        ? createBounds.default_mb
        : FALLBACK_MEMORY_BOUNDS.default_mb),
  );

  // §7 fallback gating: with the data folder unavailable a new instance would
  // be written into the temporary default root.
  const dataRootReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : get(t)(key);
  });

  // The one reason Create cannot run yet, shown beside the button. A version is
  // required for every loader: the reason always said so, and an instance made
  // without one only greets the user with "pick a version" later.
  const createDisabledReason = $derived.by(() => {
    if (dataRootReason) return dataRootReason;
    if (!draftName.trim()) return get(t)('instance.error.nameRequired');
    if (!draftMc) return get(t)('instance.error.pickMcFirst');
    // Neutral on purpose: the picker's own alert beside this names the cause
    // (no build, network, snapshot-only builds, an unreadable list).
    if (draftLoader !== 'vanilla' && !draftLoaderVersion)
      return get(t)('instance.error.loaderVersionRequired', {
        loader: displayLoader(draftLoader),
      });
    return '';
  });

  // The reason a click on Create announced. It moves from beside the button
  // into the alert while it still holds — said once, not twice — and once the
  // form moves past it the current reason is back beside the button, for good:
  // the same cause coming back later is not announced again unasked.
  let announcedReason = $state<string | null>(null);
  $effect(() => {
    if (announcedReason !== null && createDisabledReason !== announcedReason)
      announcedReason = null;
  });
  const reasonAnnounced = $derived(
    createDisabledReason !== '' && announcedReason === createDisabledReason,
  );

  // An error is about the attempt that raised it: a new choice clears it.
  $effect(() => {
    void draftMc;
    void draftLoader;
    error = null;
  });

  async function submitCreate() {
    if (createPending) return;
    // Create stays enabled so it is keyboard-reachable; a click while a reason
    // shows announces that reason instead of creating anything.
    if (createDisabledReason) {
      announcedReason = createDisabledReason;
      return;
    }
    createPending = true;
    error = null;
    try {
      const result = await commands.createInstance(
        draftName.trim(),
        draftMc,
        draftLoader,
        draftLoaderVersion,
        createHeapMb,
      );
      if (result.status !== 'ok') {
        error = instanceIpcErrorMessage(result.error);
        return;
      }
      // Make the new instance active — "I just made this thing to play it".
      const failure = await activate(result.data.id);
      if (failure === null) {
        pushSuccess(get(t)('instance.manage.createdActiveToast', { name: result.data.name }));
      } else {
        // Created, but still on the previous instance: say exactly that.
        pushSuccess(get(t)('instance.settings.createdNotSwitched', { name: result.data.name }));
        pushWarning(get(t)('instance.settings.activateFailed', { name: result.data.name }), [
          failure,
        ]);
      }
      onCreated(result.data.id);
      onClose();
    } finally {
      createPending = false;
    }
  }

  function cancel() {
    if (!createPending) onClose();
  }
</script>

<Modal
  ariaLabelledby="create-instance-title"
  onClose={cancel}
  panelClass="w-[520px] max-w-full p-5 flex flex-col"
>
  <h2 id="create-instance-title" class="text-base font-semibold text-primary mb-3">
    {$t('instance.manage.createHeading')}
  </h2>
  <label for="create-name" class="mb-1 flex justify-between text-xs text-secondary">
    <span>{$t('instance.manage.nameLabel')}</span>
    <span class="text-placeholder font-normal"
      >{$t('instance.manage.nameCounter', { count: draftName.length, max: NAME_MAX })}</span
    >
  </label>
  <input
    id="create-name"
    class="border rounded px-2 py-1 w-full mb-3"
    maxlength={NAME_MAX}
    bind:value={draftName}
    data-autofocus
  />

  <label for="create-mc-version" class="block text-xs text-secondary mb-1"
    >{$t('instance.manage.mcVersionLabel')}</label
  >
  <Select
    id="create-mc-version"
    class="w-full mb-1"
    value={draftMc}
    options={mcVersionOptions}
    onChange={(v) => (draftMc = String(v))}
  />
  <label class="text-xs flex items-center gap-1 mb-3">
    <input type="checkbox" bind:checked={showSnapshots} />
    {$t('instance.manage.showSnapshots')}
  </label>
  {#if visibleVersions.length === 0}
    <VersionListNotice
      total={versions.length}
      loading={versionList.loading}
      failure={versionList.failure}
      onReload={versionList.reload}
      pickerId="create-mc-version"
    />
  {/if}

  <LoaderPicker mc={draftMc} bind:loader={draftLoader} bind:loaderVersion={draftLoaderVersion} />

  <label for="create-memory" class="mt-3 mb-1 block text-xs text-secondary">
    {$t('instance.manage.memoryLabel', { value: formatHeapLabel($t, createHeapMb) })}
  </label>
  <!-- No onCommit: this is a draft, written once by Create. -->
  <MemorySlider
    id="create-memory"
    class="mb-1"
    warnClass="mb-3"
    reserveWarnSpace
    valueMb={createHeapMb}
    onInput={(mb) => (createHeapDraft = mb)}
  />

  <StatusMessage
    tone="danger"
    message={error ?? (reasonAnnounced ? createDisabledReason : null)}
    class="mt-2"
    dataTestid="create-instance-error"
  />

  <div class="flex items-center justify-end gap-2 mt-4">
    <!-- A visible reason instead of a tooltip on a disabled button: Create stays
         keyboard-reachable, and a click announces the same reason. -->
    {#if createDisabledReason && !reasonAnnounced}
      <span class="mr-auto text-xs text-secondary">{createDisabledReason}</span>
    {/if}
    <button type="button" class="btn-secondary btn-sm" disabled={createPending} onclick={cancel}>
      {$t('instance.manage.cancelBtn')}
    </button>
    <BusyButton
      class="btn-primary btn-sm"
      busy={createPending}
      onclick={submitCreate}
      data-testid="create-instance-submit"
    >
      {$t('instance.manage.createBtn')}
    </BusyButton>
  </div>
</Modal>
