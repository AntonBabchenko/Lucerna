<script lang="ts">
  import { Channel } from '@tauri-apps/api/core';
  import { commands } from '$lib/ipc/bindings';
  import type {
    InstanceWithStatus,
    ModpackHit,
    ModpackProgress,
    ModpackSummary,
    ProgressTick,
  } from '$lib/ipc/bindings';
  import ImportDropzone from './ImportDropzone.svelte';
  import ImportPickerDialog from './ImportPickerDialog.svelte';
  import ImportProgressView from './ImportProgressView.svelte';
  import ImportedView from './ImportedView.svelte';
  import ModpackBrowseView from './ModpackBrowseView.svelte';
  import ModpackVersionDrawer from './ModpackVersionDrawer.svelte';

  // Top-level pane wired into MainTabs. Owns:
  //   • The Browse | Imported sub-tab shell (same lazy-mount + CSS-hide
  //     pattern as the sub-3 mod browser, so search query / pagination
  //     state survives switching tabs and back).
  //   • The state machine: summary | importing | drawerHit | error.
  //   • The two `Channel<T>` instances threaded into `modpackImport`.
  //     `ImportProgressView` is render-only (Task 10's deferred decision);
  //     this component holds the latest phase + per-mod tick in `$state`
  //     and feeds them down as props.
  //
  // The picker dialog stashes the file path on the `ModpackSummary`
  // (`._path`) so the confirm step can re-pass it to `modpackImport`
  // without re-inspecting.

  let {
    instances,
    onInstanceCreated,
    onListChanged,
  }: {
    instances: InstanceWithStatus[];
    onInstanceCreated: (id: string) => void;
    onListChanged?: () => void;
  } = $props();

  type SubTab = 'browse' | 'imported';
  let activeSub = $state<SubTab>('browse');
  // Lazy-mount + CSS-hide: only render a sub-tab once it's been
  // activated; once mounted keep it in the DOM so search results,
  // pagination, filter state etc. survive switching back and forth.
  let browseEverActive = $state(true);
  let importedEverActive = $state(false);
  $effect(() => {
    if (activeSub === 'browse') browseEverActive = true;
    if (activeSub === 'imported') importedEverActive = true;
  });

  // Picker / progress / drawer state machine.
  let summary = $state<ModpackSummary | null>(null);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let drawerHit = $state<ModpackHit | null>(null);

  // Latest values pushed over the two channels during an active import.
  // Reset on each new import so a previous run's progress can't bleed
  // into the next.
  let phase = $state<ModpackProgress | null>(null);
  let modBytes = $state<ProgressTick | null>(null);

  // Hint params for `modpack_import`. Set when the user lands here from
  // the Modrinth Browse flow (so the new instance gets `mrpack_project_id`
  // + `mrpack_source = 'modrinth'` stamped onto it without a second API
  // hop on the Rust side). Drag-drop imports keep these null and the
  // orchestrator falls back to the version-id auto-lookup added in P1.
  let hintProjectId = $state<string | null>(null);
  let hintSource = $state<'modrinth' | 'curseforge' | null>(null);

  async function inspect(path: string) {
    error = null;
    const r = await commands.modpackInspect(path);
    if (r.status === 'ok') {
      // Stash the path on the summary so confirmImport can use it
      // without re-prompting the user.
      summary = { ...r.data, _path: path } as ModpackSummary & { _path: string };
    } else {
      error = String(r.error);
    }
  }

  function resetHints() {
    hintProjectId = null;
    hintSource = null;
  }

  async function confirmImport(selectedShas: string[]) {
    if (!summary) return;
    const path = (summary as ModpackSummary & { _path: string })._path;
    summary = null;
    importing = true;
    phase = null;
    modBytes = null;

    // Snapshot the hints before the await chain so they survive a
    // user-driven reset (e.g. closing the drawer mid-import) and so
    // the resetHints() in the finally branches don't race with the
    // command call.
    const pid = hintProjectId;
    const src = hintSource;

    const phaseChannel = new Channel<ModpackProgress>();
    phaseChannel.onmessage = (m) => {
      phase = m;
      if (m.phase === 'done') {
        importing = false;
        onInstanceCreated(m.instance_id);
      }
    };

    const tickChannel = new Channel<ProgressTick>();
    tickChannel.onmessage = (t) => {
      modBytes = t;
    };

    const r = await commands.modpackImport(
      path,
      selectedShas,
      true,
      pid,
      src,
      phaseChannel,
      tickChannel,
    );
    // Happy-path: the phase channel surfaces `{ phase: 'done', instance_id }`
    // and fires `onInstanceCreated` from inside `phaseChannel.onmessage`
    // above. The command return value is only consulted for the error
    // branch — the Rust side guarantees the `done` phase is emitted
    // before the command returns Ok.
    if (r.status === 'error') {
      importing = false;
      error = String(r.error);
    }
    resetHints();
  }
</script>

<div class="flex flex-col h-full">
  <div role="tablist" class="border-b flex gap-1 px-3 bg-white">
    <button
      type="button"
      role="tab"
      aria-selected={activeSub === 'browse'}
      class="px-3 py-2 text-sm border-b-2 -mb-px"
      class:border-blue-600={activeSub === 'browse'}
      class:font-semibold={activeSub === 'browse'}
      class:border-transparent={activeSub !== 'browse'}
      class:text-neutral-400={activeSub !== 'browse'}
      onclick={() => (activeSub = 'browse')}
    >
      Browse
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={activeSub === 'imported'}
      class="px-3 py-2 text-sm border-b-2 -mb-px"
      class:border-blue-600={activeSub === 'imported'}
      class:font-semibold={activeSub === 'imported'}
      class:border-transparent={activeSub !== 'imported'}
      class:text-neutral-400={activeSub !== 'imported'}
      onclick={() => (activeSub = 'imported')}
    >
      Imported
    </button>
  </div>

  <div class="flex-1 overflow-y-auto">
    {#if error}
      <div class="m-4 p-3 bg-red-50 border border-red-200 rounded text-sm text-red-900">
        {error}
      </div>
    {/if}

    {#if browseEverActive}
      <div class:hidden={activeSub !== 'browse'}>
        <div class="p-4 pb-0">
          <ImportDropzone
            onPicked={(p) => {
              // Drag-drop / file-picker imports have no Browse-flow
              // context, so any leftover hints from a previously
              // started-then-abandoned Browse import don't apply here.
              resetHints();
              void inspect(p);
            }}
            onError={(m) => (error = m)}
          />
        </div>
        <ModpackBrowseView onPickHit={(h) => (drawerHit = h)} />
      </div>
    {/if}
    {#if importedEverActive}
      <div class:hidden={activeSub !== 'imported'}>
        <ImportedView {instances} onPick={onInstanceCreated} {onListChanged} />
      </div>
    {/if}
  </div>
</div>

{#if summary}
  <ImportPickerDialog {summary} onCancel={() => (summary = null)} onConfirm={confirmImport} />
{/if}

{#if importing}
  <ImportProgressView {phase} {modBytes} />
{/if}

{#if drawerHit}
  <ModpackVersionDrawer
    hit={drawerHit}
    onClose={() => (drawerHit = null)}
    onInstall={(p) => {
      // Stash the Modrinth project_id so confirmImport can pass it
      // through to `modpack_import` as `hintProjectId`. The orchestrator
      // uses it to populate `mrpack_project_id` + `mrpack_source` on
      // the new instance without re-querying Modrinth.
      hintProjectId = drawerHit?.project_id ?? null;
      hintSource = drawerHit ? 'modrinth' : null;
      drawerHit = null;
      void inspect(p);
    }}
  />
{/if}
