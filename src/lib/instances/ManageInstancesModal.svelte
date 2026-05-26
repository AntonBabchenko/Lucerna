<script lang="ts">
  import {
    commands,
    type InstanceWithStatus,
    type LoaderKind,
    type VersionEntry,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { formatError } from '$lib/ipc/format-error';

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

  async function setMc(mc: string) {
    if (!selected) return;
    const result = await commands.setInstanceVersion(selected.id, mc);
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
  }

  async function commitLoader(kind: LoaderKind, version: string | null) {
    if (!selected) return;
    if (kind !== 'vanilla' && !selected.mc_version) {
      modalError = 'Pick a Minecraft version first';
      return;
    }
    const result = await commands.setInstanceLoader(selected.id, kind, version);
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
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
    // Escape closes the delete-confirm overlay first if it's open;
    // otherwise it closes the whole Manage modal.
    if (deleteConfirmOpen) {
      deleteConfirmOpen = false;
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
        <h2 class="font-semibold">Manage Instances</h2>
        <button class="text-muted hover:text-primary" onclick={close}>×</button>
      </header>
      <div class="flex flex-1 overflow-hidden">
        <aside class="w-[220px] border-r overflow-y-auto p-2 flex flex-col gap-1">
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
          <button
            class="mt-2 text-sm border border-dashed rounded px-2 py-1 hover:bg-subtle"
            onclick={openCreate}
          >
            + New instance
          </button>
        </aside>
        <section class="flex-1 overflow-y-auto p-4">
          {#if createMode}
            <h3 class="font-semibold mb-3">New instance</h3>
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
              <button class="border rounded px-3 py-1 text-sm" onclick={() => (createMode = false)}>
                Cancel
              </button>
              <button
                class="bg-accent text-white rounded px-3 py-1 text-sm hover:bg-accent disabled:bg-muted disabled:cursor-not-allowed"
                disabled={!!createDisabledReason}
                title={createDisabledReason}
                onclick={submitCreate}
              >
                Create
              </button>
            </div>
          {:else if selected}
            <h3 class="font-semibold mb-3">
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

            <div class="flex items-center justify-between pt-3 border-t">
              <button class="border rounded px-3 py-1 text-xs" onclick={openFolder}>
                📁 Open folder
              </button>
              <div class="flex gap-2">
                <button
                  class="border border-danger text-danger rounded px-3 py-1 text-xs hover:bg-danger/10 disabled:opacity-40 disabled:cursor-not-allowed"
                  disabled={instances.length <= 1}
                  title={instances.length <= 1 ? 'Cannot delete the last instance' : ''}
                  onclick={() => (deleteConfirmOpen = true)}
                >
                  🗑 Delete
                </button>
                <button
                  class="bg-accent text-white rounded px-3 py-1 text-xs hover:bg-accent"
                  onclick={close}
                >
                  Done
                </button>
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
  </div>

  {#if deleteConfirmOpen && selected}
    <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-[60]">
      <div class="bg-surface rounded-lg shadow-xl w-[440px] p-5 flex flex-col gap-3">
        <h3 class="font-semibold text-base">Delete instance?</h3>
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
            class="border rounded px-3 py-1 text-sm"
            onclick={() => (deleteConfirmOpen = false)}
          >
            Cancel
          </button>
          <button
            class="bg-danger text-white rounded px-3 py-1 text-sm hover:bg-danger"
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
{/if}
