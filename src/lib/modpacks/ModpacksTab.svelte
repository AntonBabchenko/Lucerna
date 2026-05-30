<script lang="ts">
  import { Channel } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import type {
    InstanceWithStatus,
    ModpackHit,
    ModpackProgress,
    ModpackSummary,
    ProgressTick,
  } from '$lib/ipc/bindings';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { droppedModpack, modpacksNav, dragActive } from '$lib/settings/state.svelte';
  import ImportPickerDialog from './ImportPickerDialog.svelte';
  import ImportProgressView from './ImportProgressView.svelte';
  import ImportedView from './ImportedView.svelte';
  import ModpackBrowseView from './ModpackBrowseView.svelte';
  import ModpackDetailModal from './ModpackDetailModal.svelte';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { MODPACKS_STEPS } from '$lib/onboarding/contextual-tours';

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

  // When the Overview indicator requests a pack drawer, switch to the
  // Imported sub-tab so ImportedView (which opens the drawer) is mounted.
  $effect(() => {
    if (modpacksNav.value !== null) {
      activeSub = 'imported';
      importedEverActive = true;
    }
  });

  // A modpack dropped on the Modpacks view arrives via the
  // droppedModpack rune. Consume and reset immediately. A drag-drop
  // import has no Browse-flow context, so clear any stale hints first.
  $effect(() => {
    const v = droppedModpack.value;
    if (v !== null) {
      droppedModpack.value = null;
      resetHints();
      void inspect(v);
    }
  });

  // Window-level drag-drop listener scoped to this view's lifetime —
  // Modpacks moved out of MainTabs into the sidebar, so MainTabs no
  // longer routes .mrpack/.zip drops. The listener (re)mounts when
  // the user opens the Modpacks view and tears down on close, so
  // there's never more than one active.
  onMount(() => {
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      const t = (event as { payload: { type: string; paths?: string[] } }).payload.type;
      if (t === 'enter' || t === 'over') {
        dragActive.value = true;
      } else if (t === 'leave') {
        dragActive.value = false;
      } else if (t === 'drop') {
        dragActive.value = false;
        const paths =
          (event as { payload: { type: string; paths?: string[] } }).payload.paths ?? [];
        const pack = paths.find((p) => /\.(mrpack|zip)$/i.test(p));
        if (pack) droppedModpack.value = pack;
      }
    });
    return () => {
      void pending.then((un) => un());
    };
  });

  async function importFromFile() {
    const r = await openFile({
      multiple: false,
      filters: [{ name: 'Modpack', extensions: ['mrpack', 'zip'] }],
    });
    if (typeof r === 'string') {
      resetHints();
      void inspect(r);
    }
  }

  // Picker / progress / drawer state machine.
  let summary = $state<ModpackSummary | null>(null);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let drawerHit = $state<ModpackHit | null>(null);
  // MC version filter the user had in the toolbar when they clicked the
  // card. Null = no filter → drawer lists every version of the pack.
  // Set = drawer only shows versions matching that MC, so the visible
  // list reflects the filtered grid the user came from.
  let drawerMcFilter = $state<string | null>(null);

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
  // Modrinth version id of the picked version (Browse flow). Threaded to
  // `modpack_import` so the new instance stores `mrpack_version_id` — the
  // stable identifier the update flow compares against.
  let hintVersionId = $state<string | null>(null);

  async function inspect(path: string) {
    error = null;
    const r = await commands.modpackInspect(path);
    if (r.status === 'ok') {
      // Stash the path on the summary so confirmImport can use it
      // without re-prompting the user.
      summary = { ...r.data, _path: path } as ModpackSummary & { _path: string };
    } else {
      error = formatError(r.error);
    }
  }

  function resetHints() {
    hintProjectId = null;
    hintSource = null;
    hintVersionId = null;
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
    const vid = hintVersionId;

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
      vid,
      phaseChannel,
      tickChannel,
    );
    // Happy-path: the phase channel surfaces `{ phase: 'done', instance_id }`
    // and fires `onInstanceCreated` from inside `phaseChannel.onmessage`
    // above. The command return value is only consulted for the error
    // branch — the Rust side guarantees the `done` phase is emitted
    // before the command returns Ok.
    if (r.status === 'ok') {
      pushSuccess(`Imported ${r.data.name}`);
    } else {
      importing = false;
      if (r.error.kind === 'modpack_partial_failure') {
        pushWarning(
          `Modpack imported — ${r.error.failed.length} mod(s) failed`,
          r.error.failed.map(([p]) => p.split('/').pop() ?? p),
        );
      } else {
        pushWarning('Modpack import failed', [formatError(r.error)]);
      }
    }
    resetHints();
  }
</script>

<div class="flex flex-col h-full">
  <div role="tablist" class="border-b flex gap-1 px-3 bg-surface" data-tour-ctx="modpacks-tabs">
    <button
      type="button"
      role="tab"
      aria-selected={activeSub === 'browse'}
      class="px-3 py-2 text-sm border-b-2 -mb-px"
      class:border-accent={activeSub === 'browse'}
      class:text-primary={activeSub === 'browse'}
      class:font-semibold={activeSub === 'browse'}
      class:border-transparent={activeSub !== 'browse'}
      class:text-placeholder={activeSub !== 'browse'}
      onclick={() => (activeSub = 'browse')}
    >
      Browse
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={activeSub === 'imported'}
      class="px-3 py-2 text-sm border-b-2 -mb-px"
      class:border-accent={activeSub === 'imported'}
      class:text-primary={activeSub === 'imported'}
      class:font-semibold={activeSub === 'imported'}
      class:border-transparent={activeSub !== 'imported'}
      class:text-placeholder={activeSub !== 'imported'}
      onclick={() => (activeSub = 'imported')}
    >
      Imported
    </button>
  </div>

  <div class="px-4 pt-3" data-tour-ctx="modpacks-dropzone">
    <FileDropzone
      label="Drop a .mrpack or .zip here to import — or click to browse"
      onClick={importFromFile}
    />
  </div>

  <ContextualTour id="modpacks" steps={MODPACKS_STEPS} />

  <div class="flex-1 overflow-y-auto">
    {#if error}
      <div class="m-4 p-3 bg-danger-bg border border-danger rounded text-sm text-danger">
        {error}
      </div>
    {/if}

    {#if browseEverActive}
      <div class:hidden={activeSub !== 'browse'}>
        <ModpackBrowseView
          onPickHit={(h, mc) => {
            drawerHit = h;
            drawerMcFilter = mc;
          }}
        />
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
  <ModpackDetailModal
    hit={drawerHit}
    mcFilter={drawerMcFilter}
    onClose={() => (drawerHit = null)}
    onInstall={(p, vid) => {
      // Stash the Modrinth project_id + version id so confirmImport can
      // pass them through to `modpack_import`. The orchestrator uses them
      // to populate `mrpack_project_id` / `mrpack_source` /
      // `mrpack_version_id` on the new instance without re-querying.
      hintProjectId = drawerHit?.project_id ?? null;
      hintSource = drawerHit?.source ?? null;
      hintVersionId = vid;
      drawerHit = null;
      void inspect(p);
    }}
  />
{/if}
