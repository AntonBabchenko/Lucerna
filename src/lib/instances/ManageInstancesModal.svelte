<script lang="ts">
  import {
    commands,
    type InstanceWithStatus,
    type LoaderKind,
    type LoaderVersion,
    type VersionEntry,
    type Error as IpcError,
  } from '$lib/ipc/bindings';

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

  const LOADER_KINDS: LoaderKind[] = ['vanilla', 'fabric', 'quilt', 'forge', 'neoforge'];

  let selectedId = $state<string | null>(null);
  let selected = $derived(instances.find((i) => i.id === selectedId) ?? null);
  let createMode = $state(false);
  let modalError = $state<string | null>(null);

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
  let draftLoaderVersions = $state<LoaderVersion[]>([]);
  let draftLoaderVersion = $state<string | null>(null);

  // Detail form state — reactive to `selected`.
  let nameDraft = $state('');
  let loaderVersionsDetail = $state<LoaderVersion[]>([]);

  $effect(() => {
    if (selected) {
      nameDraft = selected.name;
      if (selected.loader !== 'vanilla' && selected.mc_version) {
        void loadLoaderVersionsFor(selected.loader, selected.mc_version, 'detail');
      }
    }
  });

  // Create-form mirror of the home-screen effect: refetch + auto-pick a
  // recommended loader version whenever the user changes loader or MC in
  // the draft, regardless of order. Without this the dropdown stays
  // empty if the user clicks Fabric/Quilt/Forge before picking an MC
  // version, and `draftLoaderVersion` stays null — submitCreate would
  // then create an instance in a broken state that errors with
  // `NoVersionSelected` at install time.
  $effect(() => {
    if (!createMode) return;
    const kind = draftLoader;
    const mc = draftMc;
    if (kind === 'vanilla' || !mc) {
      draftLoaderVersions = [];
      draftLoaderVersion = null;
      return;
    }
    void loadLoaderVersionsFor(kind, mc, 'create');
  });

  function ipcErrorMessage(e: IpcError): string {
    return JSON.stringify(e);
  }

  async function loadLoaderVersionsFor(
    loader: LoaderKind,
    mc: string,
    target: 'detail' | 'create',
  ) {
    if (loader === 'vanilla' || !mc) {
      if (target === 'create') draftLoaderVersions = [];
      else loaderVersionsDetail = [];
      return;
    }
    const list =
      loader === 'fabric'
        ? await commands.listFabricLoaders(mc)
        : loader === 'quilt'
          ? await commands.listQuiltLoaders(mc)
          : loader === 'neoforge'
            ? await commands.listNeoforgeLoaders(mc)
            : await commands.listForgeLoaders(mc);
    if (list.status === 'ok') {
      if (target === 'create') {
        draftLoaderVersions = list.data;
        const stable = list.data.find((l) => l.stable);
        draftLoaderVersion = (stable ?? list.data[0])?.version ?? null;
      } else {
        loaderVersionsDetail = list.data;
      }
    } else {
      modalError = ipcErrorMessage(list.error);
    }
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
      // The $effect that loads loader versions hasn't picked a default
      // yet (still in flight, or list_loaders returned empty for this
      // loader/MC combo). Either way: refuse to persist a broken
      // loader/null-version pair — the user must resolve it first.
      modalError = `${draftLoader} has no compatible version for Minecraft ${draftMc}`;
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

  async function setLoader(kind: LoaderKind) {
    if (!selected) return;
    let lv: string | null = null;
    if (kind !== 'vanilla') {
      if (!selected.mc_version) {
        modalError = 'Pick a Minecraft version first';
        return;
      }
      await loadLoaderVersionsFor(kind, selected.mc_version, 'detail');
      const stable = loaderVersionsDetail.find((l) => l.stable);
      lv = (stable ?? loaderVersionsDetail[0])?.version ?? null;
      if (lv === null) {
        // list_loaders returned empty (or errored — modalError is
        // already set). Refuse to persist a broken loader/null-version
        // combination; the user can pick a different loader or MC.
        if (!modalError) {
          modalError = `${kind} does not support Minecraft ${selected.mc_version}`;
        }
        return;
      }
    }
    const result = await commands.setInstanceLoader(selected.id, kind, lv);
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
  }

  async function setLoaderVersion(v: string) {
    if (!selected) return;
    const result = await commands.setInstanceLoader(selected.id, selected.loader, v);
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
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div class="fixed inset-0 bg-black/40 flex items-center justify-center z-50">
    <div class="bg-white rounded-lg shadow-xl w-[760px] max-h-[80vh] overflow-hidden flex flex-col">
      <header class="flex items-center justify-between px-4 py-2 border-b">
        <h2 class="font-semibold">Manage Instances</h2>
        <button class="text-neutral-500 hover:text-neutral-800" onclick={close}>×</button>
      </header>
      <div class="flex flex-1 overflow-hidden">
        <aside class="w-[220px] border-r overflow-y-auto p-2 flex flex-col gap-1">
          {#each instances as i}
            <button
              class="text-left px-2 py-1 rounded text-sm hover:bg-neutral-100"
              class:bg-blue-100={i.id === selectedId}
              onclick={() => {
                createMode = false;
                selectedId = i.id;
              }}
            >
              <div class="font-medium">
                {i.ready ? '✓' : '↓'}
                {i.name}
                {#if i.id === activeInstance?.id}
                  <span class="text-xs text-neutral-500">(active)</span>
                {/if}
              </div>
              <div class="text-xs text-neutral-500">
                {i.loader} · {i.mc_version || '(pick MC)'}
              </div>
            </button>
          {/each}
          <button
            class="mt-2 text-sm border border-dashed rounded px-2 py-1 hover:bg-neutral-100"
            onclick={openCreate}
          >
            + New instance
          </button>
        </aside>
        <section class="flex-1 overflow-y-auto p-4">
          {#if createMode}
            <h3 class="font-semibold mb-3">New instance</h3>
            <label class="block text-xs uppercase text-neutral-600 mb-1">Name</label>
            <input class="border rounded px-2 py-1 w-full mb-3" bind:value={draftName} />

            <label class="block text-xs uppercase text-neutral-600 mb-1">Minecraft version</label>
            <select
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

            <label class="block text-xs uppercase text-neutral-600 mb-1">Loader</label>
            <div class="flex gap-1 mb-3">
              {#each LOADER_KINDS as lk}
                <button
                  class="flex-1 border rounded px-2 py-1 text-xs"
                  class:bg-blue-600={draftLoader === lk}
                  class:text-white={draftLoader === lk}
                  onclick={async () => {
                    draftLoader = lk;
                    if (lk !== 'vanilla' && draftMc) {
                      await loadLoaderVersionsFor(lk, draftMc, 'create');
                    }
                  }}>{lk}</button
                >
              {/each}
            </div>

            {#if draftLoader !== 'vanilla' && draftLoaderVersions.length > 0}
              <label class="block text-xs uppercase text-neutral-600 mb-1">Loader version</label>
              <select
                class="border rounded px-2 py-1 w-full mb-3"
                value={draftLoaderVersion ?? ''}
                onchange={(e) =>
                  (draftLoaderVersion = (e.currentTarget as HTMLSelectElement).value)}
              >
                {#each draftLoaderVersions as lv}
                  <option value={lv.version}>
                    {lv.version}{lv.stable ? ' (recommended)' : ''}
                  </option>
                {/each}
              </select>
            {/if}

            <div class="flex justify-end gap-2 mt-4">
              <button class="border rounded px-3 py-1 text-sm" onclick={() => (createMode = false)}>
                Cancel
              </button>
              <button
                class="bg-blue-600 text-white rounded px-3 py-1 text-sm hover:bg-blue-700"
                onclick={submitCreate}
              >
                Create
              </button>
            </div>
          {:else if selected}
            <h3 class="font-semibold mb-3">
              {selected.name}
              {#if selected.id === activeInstance?.id}<span class="text-xs text-neutral-500"
                  >(active)</span
                >{/if}
            </h3>

            <label class="block text-xs uppercase text-neutral-600 mb-1">Name</label>
            <input
              class="border rounded px-2 py-1 w-full mb-3"
              bind:value={nameDraft}
              onblur={commitName}
            />

            <label class="block text-xs uppercase text-neutral-600 mb-1">Minecraft version</label>
            <select
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

            <label class="block text-xs uppercase text-neutral-600 mb-1">Loader</label>
            <div class="flex gap-1 mb-3">
              {#each LOADER_KINDS as lk}
                <button
                  class="flex-1 border rounded px-2 py-1 text-xs"
                  class:bg-blue-600={selected.loader === lk}
                  class:text-white={selected.loader === lk}
                  onclick={() => setLoader(lk)}>{lk}</button
                >
              {/each}
            </div>

            {#if selected.loader !== 'vanilla' && loaderVersionsDetail.length > 0}
              <label class="block text-xs uppercase text-neutral-600 mb-1">Loader version</label>
              <select
                class="border rounded px-2 py-1 w-full mb-3"
                value={selected.loader_version ?? ''}
                onchange={(e) => setLoaderVersion((e.currentTarget as HTMLSelectElement).value)}
              >
                {#each loaderVersionsDetail as lv}
                  <option value={lv.version}>
                    {lv.version}{lv.stable ? ' (recommended)' : ''}
                  </option>
                {/each}
              </select>
            {/if}

            <label class="block text-xs uppercase text-neutral-600 mb-1">
              Memory (max heap): {selected.max_heap_mb} MB
            </label>
            <input
              type="range"
              min="1024"
              max="8192"
              step="256"
              value={selected.max_heap_mb}
              oninput={(e) => setMemory(parseInt((e.currentTarget as HTMLInputElement).value, 10))}
              class="w-full mb-3"
            />

            <label class="block text-xs uppercase text-neutral-600 mb-1">Extra JVM args</label>
            <input
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
                  class="border border-red-300 text-red-700 rounded px-3 py-1 text-xs hover:bg-red-50 disabled:opacity-40 disabled:cursor-not-allowed"
                  disabled={instances.length <= 1}
                  title={instances.length <= 1 ? 'Cannot delete the last instance' : ''}
                  onclick={deleteSelected}
                >
                  🗑 Delete
                </button>
                <button
                  class="bg-blue-600 text-white rounded px-3 py-1 text-xs hover:bg-blue-700"
                  onclick={close}
                >
                  Done
                </button>
              </div>
            </div>
          {:else}
            <p class="text-neutral-500 text-sm">
              Pick an instance on the left, or click + New instance.
            </p>
          {/if}

          {#if modalError}
            <p class="text-xs text-red-700 mt-3">{modalError}</p>
          {/if}
        </section>
      </div>
    </div>
  </div>
{/if}
