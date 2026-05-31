<script lang="ts">
  import {
    commands,
    type InstanceWithStatus,
    type LoaderKind,
    type VersionEntry,
    type Error as IpcError,
    type ModCompat,
  } from '$lib/ipc/bindings';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { loaderOutcomeToast, compatSummary } from '$lib/instances/integrity-messages';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { formatError } from '$lib/ipc/format-error';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { MANAGE_STEPS } from '$lib/onboarding/contextual-tours';
  import CloseButton from '$lib/ui/CloseButton.svelte';

  let {
    open = $bindable(),
    instances = $bindable<InstanceWithStatus[]>(),
    activeInstance = $bindable<InstanceWithStatus | null>(),
    versions,
    onChanged,
  }: {
    open: boolean;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    versions: VersionEntry[];
    onChanged: () => void;
  } = $props();

  let selectedId = $state<string | null>(null);
  let selected = $derived(instances.find((i) => i.id === selectedId) ?? null);
  let createMode = $state(false);

  // When the modal opens, default the selection to the currently-active
  // instance (the one the user is playing on the main view). Otherwise
  // the detail panel either shows empty state (selectedId=null) or
  // whatever was selected in a previous session — both surprising. The
  // user expects to manage the instance they just had open.
  $effect(() => {
    if (open && selectedId === null) {
      selectedId = activeInstance?.id ?? instances[0]?.id ?? null;
    }
  });
  let modalError = $state<string | null>(null);
  let deleteConfirmOpen = $state(false);

  // Mod-compat summary for the current instance after an MC/loader change.
  let compatRows = $state<ModCompat[] | null>(null);
  // Reset compat rows when the selected instance changes.
  $effect(() => {
    void selectedId;
    compatRows = null;
  });

  // Pending pack-detach confirm state.
  // Holds { kind: 'mc', value: string } or { kind: 'loader', loaderKind, loaderVersion }
  // when awaiting the user's decision.
  type PendingChange =
    | { kind: 'mc'; value: string }
    | { kind: 'loader'; loaderKind: LoaderKind; loaderVersion: string | null };
  let pendingChange = $state<PendingChange | null>(null);

  // Snapshot toggle for the MC version pickers. Off by default —
  // most users want stable releases. Shared across the create form
  // and the detail editor so flipping it once applies to both.
  let showSnapshots = $state(false);
  let visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );

  // Create form state.
  let draftName = $state('');
  let draftMc = $state('');
  let draftLoader = $state<LoaderKind>('vanilla');
  let draftLoaderVersion = $state<string | null>(null);

  // Detail form state — reactive to `selected`.
  let nameDraft = $state('');

  $effect(() => {
    if (selected) {
      nameDraft = selected.name;
    }
  });

  // Auto-clear stale modalError when the user navigates away from
  // whatever caused it — switching instances, opening/closing the
  // create form, or picking a different MC/loader in create draft.
  // Without this, a "quilt has no version for 26.1.2" error from a
  // previous attempt would linger on top of an unrelated screen.
  let createDisabledReason = $derived.by(() => {
    if (!createMode) return '';
    if (!draftName.trim()) return 'Enter a name';
    if (!draftMc) return 'Pick a Minecraft version first';
    if (draftLoader !== 'vanilla' && !draftLoaderVersion)
      return `${displayLoader(draftLoader)} does not support Minecraft ${draftMc} — try another version or loader`;
    return '';
  });
  $effect(() => {
    // Track everything that should reset the error:
    void selectedId;
    void createMode;
    void draftMc;
    void draftLoader;
    modalError = null;
  });

  function ipcErrorMessage(e: IpcError): string {
    // Modal-local shorter wording for the two name-validation cases (the
    // modal context makes "Instance" redundant). Everything else
    // delegates to the shared formatError so no IPC variant leaks raw
    // JSON if e.g. openInstanceFolder returns Error::Io.
    if (e.kind === 'instance_name_empty') return 'Name cannot be empty';
    if (e.kind === 'instance_name_too_long')
      return `Name is too long: ${e.actual}/${e.max} characters`;
    return formatError(e);
  }

  function openCreate() {
    createMode = true;
    draftName = '';
    draftMc = '';
    draftLoader = 'vanilla';
    draftLoaderVersion = null;
    modalError = null;
  }

  async function submitCreate() {
    if (!draftName.trim()) {
      modalError = 'Name is required';
      return;
    }
    if (draftLoader !== 'vanilla' && !draftMc) {
      modalError = 'Pick a Minecraft version first';
      return;
    }
    if (draftLoader !== 'vanilla' && !draftLoaderVersion) {
      // Belt-and-braces: the Create button is also disabled in this
      // state via createDisabledReason. This branch catches the
      // in-flight race where load() hasn't resolved yet.
      modalError = `${displayLoader(draftLoader)} does not support Minecraft ${draftMc} — try another version or loader`;
      return;
    }
    const result = await commands.createInstance(
      draftName.trim(),
      draftMc,
      draftLoader,
      draftLoaderVersion,
    );
    if (result.status === 'ok') {
      createMode = false;
      // Make the newly created instance active — matches user intent
      // ("I just made this thing to play it"). Editing an existing
      // non-active instance still leaves the active unchanged.
      await commands.setActiveInstance(result.data.id);
      onChanged();
      selectedId = result.data.id;
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  async function commitName() {
    if (!selected || nameDraft === selected.name) return;
    if (!nameDraft.trim()) return;
    const result = await commands.setInstanceName(selected.id, nameDraft.trim());
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
  }

  async function runModCompatCheck() {
    if (!selected) return;
    try {
      const r = await commands.checkInstanceModCompat(
        selected.id,
        selected.mc_version,
        selected.loader,
      );
      if (r.status === 'ok') compatRows = r.data;
      // On error: swallow gracefully — compat check is best-effort UX.
    } catch {
      // Unexpected throw: silently ignore.
    }
  }

  async function applyMcChange(mc: string) {
    if (!selected) return;
    const result = await commands.changeInstanceMc(selected.id, mc);
    if (result.status === 'ok') {
      const toast = loaderOutcomeToast(result.data.loader_outcome, mc);
      if (toast?.kind === 'success') pushSuccess(toast.text);
      else if (toast?.kind === 'warning') pushWarning(toast.text);
      onChanged();
      await runModCompatCheck();
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  async function setMc(mc: string) {
    if (!selected) return;
    if (selected.mrpack_name) {
      pendingChange = { kind: 'mc', value: mc };
      return;
    }
    await applyMcChange(mc);
  }

  async function applyLoaderChange(kind: LoaderKind, version: string | null) {
    if (!selected) return;
    if (kind !== 'vanilla' && !selected.mc_version) {
      modalError = 'Pick a Minecraft version first';
      return;
    }
    const result = await commands.setInstanceLoader(selected.id, kind, version);
    if (result.status === 'ok') {
      onChanged();
      await runModCompatCheck();
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  async function commitLoader(kind: LoaderKind, version: string | null) {
    if (!selected) return;
    if (selected.mrpack_name) {
      pendingChange = { kind: 'loader', loaderKind: kind, loaderVersion: version };
      return;
    }
    await applyLoaderChange(kind, version);
  }

  async function confirmDetachAndContinue() {
    if (!selected || !pendingChange) return;
    const change = pendingChange;
    pendingChange = null;
    const detachResult = await commands.detachInstancePack(selected.id);
    if (detachResult.status === 'error') {
      modalError = ipcErrorMessage(detachResult.error);
      return;
    }
    // After detach onChanged() refreshes the instance list; but we need to
    // apply the change against the updated selected. Trigger the change directly.
    onChanged();
    if (change.kind === 'mc') {
      await applyMcChange(change.value);
    } else {
      await applyLoaderChange(change.loaderKind, change.loaderVersion);
    }
  }

  async function keepAndContinue() {
    if (!selected || !pendingChange) return;
    const change = pendingChange;
    pendingChange = null;
    if (change.kind === 'mc') {
      await applyMcChange(change.value);
    } else {
      await applyLoaderChange(change.loaderKind, change.loaderVersion);
    }
  }

  function cancelPending() {
    pendingChange = null;
  }

  async function setMemory(mb: number) {
    if (!selected) return;
    const result = await commands.setInstanceMemory(selected.id, mb);
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
  }

  async function setJvmArgs(args: string) {
    if (!selected) return;
    const result = await commands.setInstanceJvmArgs(selected.id, args);
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
  }

  async function openFolder() {
    if (!selected) return;
    const result = await commands.openInstanceFolder(selected.id);
    if (result.status === 'error') modalError = ipcErrorMessage(result.error);
  }

  async function deleteSelected() {
    if (!selected) return;
    if (instances.length <= 1) return; // belt-and-braces; the button is also disabled
    const result = await commands.deleteInstance(selected.id);
    if (result.status === 'ok') {
      selectedId = null;
      onChanged();
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  function close() {
    open = false;
    // Reset so the next open re-derives selectedId from activeInstance
    // (see the open-detection $effect above). Without this, closing
    // the modal then switching the active instance on the main view
    // and reopening would still surface the previously-selected
    // instance, not the new active one.
    selectedId = null;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    // Dismiss overlays in reverse stacking order before closing the modal.
    if (deleteConfirmOpen) {
      deleteConfirmOpen = false;
    } else if (pendingChange !== null) {
      pendingChange = null;
    } else {
      close();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div class="fixed inset-0 bg-black/40 flex items-center justify-center z-50">
    <div
      class="bg-surface rounded-lg shadow-xl w-[760px] max-h-[80vh] overflow-hidden flex flex-col"
    >
      <header class="flex items-center justify-between px-4 py-2 border-b">
        <h2 class="font-semibold text-primary">Manage Instances</h2>
        <CloseButton onClick={close} ariaLabel="Close manage instances" />
      </header>
      <div class="flex flex-1 overflow-hidden">
        <aside
          class="w-[220px] border-r overflow-y-auto p-2 flex flex-col gap-1"
          data-tour-ctx="manage-list"
        >
          {#each instances as i}
            <button
              class="text-left px-2 py-1 rounded text-sm hover:bg-subtle"
              class:bg-accent-soft={i.id === selectedId}
              onclick={() => {
                createMode = false;
                selectedId = i.id;
              }}
            >
              <div class="font-medium">
                {i.ready ? '✓' : '↓'}
                {i.name}
                {#if i.id === activeInstance?.id}
                  <span class="text-xs text-muted">(active)</span>
                {/if}
              </div>
              <div class="text-xs text-muted">
                {displayLoader(i.loader)} · {i.mc_version || '(pick MC)'}
              </div>
            </button>
          {/each}
          <button type="button" class="mt-2 btn-primary btn-sm w-full" onclick={openCreate}>
            + New instance
          </button>
        </aside>
        <section class="flex-1 overflow-y-auto p-4" data-tour-ctx="manage-form">
          {#if createMode}
            <h3 class="font-semibold text-primary mb-3">New instance</h3>
            <label
              for="create-name"
              class="block text-xs uppercase text-secondary mb-1 flex justify-between"
            >
              <span>Name</span>
              <span class="text-placeholder normal-case font-normal">{draftName.length}/32</span>
            </label>
            <input
              id="create-name"
              class="border rounded px-2 py-1 w-full mb-3"
              maxlength="32"
              bind:value={draftName}
            />

            <label for="create-mc-version" class="block text-xs uppercase text-secondary mb-1"
              >Minecraft version</label
            >
            <select
              id="create-mc-version"
              class="border rounded px-2 py-1 w-full mb-1"
              value={draftMc}
              onchange={(e) => (draftMc = (e.currentTarget as HTMLSelectElement).value)}
            >
              <option value="">-- Choose MC version --</option>
              {#each visibleVersions as v}
                <option value={v.id}>{v.id}</option>
              {/each}
            </select>
            <label class="text-xs flex items-center gap-1 mb-3">
              <input type="checkbox" bind:checked={showSnapshots} />
              Show snapshots
            </label>

            <LoaderPicker
              mc={draftMc}
              bind:loader={draftLoader}
              bind:loaderVersion={draftLoaderVersion}
            />

            <div class="flex justify-end gap-2 mt-4">
              <button
                type="button"
                class="btn-secondary btn-sm"
                onclick={() => (createMode = false)}
              >
                Cancel
              </button>
              <button
                type="button"
                class="btn-primary btn-sm"
                disabled={!!createDisabledReason}
                title={createDisabledReason}
                onclick={submitCreate}
              >
                Create
              </button>
            </div>
          {:else if selected}
            <h3 class="font-semibold text-primary mb-3">
              {selected.name}
              {#if selected.id === activeInstance?.id}<span class="text-xs text-muted"
                  >(active)</span
                >{/if}
            </h3>

            <label
              for="detail-name"
              class="block text-xs uppercase text-secondary mb-1 flex justify-between"
            >
              <span>Name</span>
              <span class="text-placeholder normal-case font-normal">{nameDraft.length}/32</span>
            </label>
            <input
              id="detail-name"
              class="border rounded px-2 py-1 w-full mb-3"
              maxlength="32"
              bind:value={nameDraft}
              onblur={commitName}
            />

            <label for="detail-mc-version" class="block text-xs uppercase text-secondary mb-1"
              >Minecraft version</label
            >
            <select
              id="detail-mc-version"
              class="border rounded px-2 py-1 w-full mb-1"
              value={selected.mc_version}
              onchange={(e) => setMc((e.currentTarget as HTMLSelectElement).value)}
            >
              <option value="">-- Choose MC version --</option>
              {#each visibleVersions as v}
                <option value={v.id}>{v.id}</option>
              {/each}
            </select>
            <label class="text-xs flex items-center gap-1 mb-3">
              <input type="checkbox" bind:checked={showSnapshots} />
              Show snapshots
            </label>

            <LoaderPicker
              mc={selected.mc_version}
              loader={selected.loader}
              loaderVersion={selected.loader_version}
              onchange={async (l, v) => {
                if (l !== selected!.loader || v !== selected!.loader_version) {
                  await commitLoader(l, v);
                }
              }}
            />

            {#if compatRows !== null && compatSummary(compatRows) !== null}
              <p
                class="text-xs text-warning-text bg-warning-bg border border-warning-text/30 rounded px-2 py-1.5 mt-2 mb-1"
              >
                ⚠ {compatSummary(compatRows)}
              </p>
            {/if}

            <label for="detail-memory" class="block text-xs uppercase text-secondary mb-1">
              Memory (max heap): {selected.max_heap_mb} MB
            </label>
            <input
              id="detail-memory"
              type="range"
              min="1024"
              max="8192"
              step="256"
              value={selected.max_heap_mb}
              oninput={(e) => setMemory(parseInt((e.currentTarget as HTMLInputElement).value, 10))}
              class="w-full mb-3"
            />

            <label for="detail-jvm-args" class="block text-xs uppercase text-secondary mb-1"
              >Extra JVM args</label
            >
            <input
              id="detail-jvm-args"
              class="border rounded px-2 py-1 w-full mb-3 font-mono text-xs"
              placeholder="-XX:+UseG1GC -XX:MaxGCPauseMillis=200"
              value={selected.extra_jvm_args}
              onchange={(e) => setJvmArgs((e.currentTarget as HTMLInputElement).value)}
            />

            <div
              class="flex items-center justify-between pt-3 border-t"
              data-tour-ctx="manage-actions"
            >
              <button
                type="button"
                class="btn-ghost-danger"
                disabled={instances.length <= 1}
                title={instances.length <= 1 ? 'Cannot delete the last instance' : ''}
                onclick={() => (deleteConfirmOpen = true)}
              >
                🗑 Delete instance
              </button>
              <div class="flex gap-2">
                <button type="button" class="btn-secondary btn-sm" onclick={openFolder}>
                  📁 Open folder
                </button>
                <button type="button" class="btn-primary btn-sm" onclick={close}> Done </button>
              </div>
            </div>
          {:else}
            <p class="text-muted text-sm">Pick an instance on the left, or click + New instance.</p>
          {/if}

          {#if modalError}
            <p class="text-xs text-danger mt-3">{modalError}</p>
          {/if}
        </section>
      </div>
    </div>
    <ContextualTour id="manage" steps={MANAGE_STEPS} />
  </div>

  {#if deleteConfirmOpen && selected}
    <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]">
      <div class="bg-surface rounded-lg shadow-xl w-[440px] p-5 flex flex-col gap-3">
        <h3 class="font-semibold text-primary text-base">Delete instance?</h3>
        <p class="text-sm text-secondary">
          Delete <span class="font-mono font-semibold">{selected.name}</span>?
        </p>
        <p class="text-sm text-secondary">
          This permanently removes the instance directory including its
          <span class="font-mono">.minecraft/</span> folder — saved worlds, installed mods, configs, resource
          packs, screenshots. This cannot be undone.
        </p>
        <div class="flex justify-end gap-2 mt-2">
          <button
            type="button"
            class="btn-secondary btn-sm"
            onclick={() => (deleteConfirmOpen = false)}
          >
            Cancel
          </button>
          <button
            type="button"
            class="btn-danger btn-sm"
            onclick={async () => {
              deleteConfirmOpen = false;
              await deleteSelected();
            }}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if pendingChange !== null && selected}
    <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]">
      <div class="bg-surface rounded-lg shadow-xl w-[460px] p-5 flex flex-col gap-3">
        <h3 class="font-semibold text-primary text-base">Modpack instance</h3>
        <p class="text-sm text-secondary">
          This instance was imported from
          <span class="font-semibold">{selected.mrpack_name}</span>. Changing its
          {pendingChange.kind === 'mc' ? 'Minecraft version' : 'loader'} detaches it from the pack.
        </p>
        <div class="flex justify-end gap-2 mt-2">
          <button type="button" class="btn-secondary btn-sm" onclick={cancelPending}>
            Cancel
          </button>
          <button type="button" class="btn-secondary btn-sm" onclick={keepAndContinue}>
            Keep &amp; continue
          </button>
          <button type="button" class="btn-primary btn-sm" onclick={confirmDetachAndContinue}>
            Detach &amp; continue
          </button>
        </div>
      </div>
    </div>
  {/if}
{/if}
