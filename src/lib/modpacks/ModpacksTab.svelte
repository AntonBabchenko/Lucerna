<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import type {
    InstanceWithStatus,
    ModpackHit,
    ModpackSummary,
    ModSource,
  } from '$lib/ipc/bindings';
  import type { ModpackImportRequest } from './import-request';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { droppedModpack, modpacksNav, dragActive } from '$lib/settings/state.svelte';
  import ImportPickerDialog from './ImportPickerDialog.svelte';
  import ImportedView from './ImportedView.svelte';
  import ModpackBrowseView from './ModpackBrowseView.svelte';
  import ModpackDetailModal from './ModpackDetailModal.svelte';
  import FileDropzone from '$lib/mods/FileDropzone.svelte';
  import SourcePicker from '$lib/mods/SourcePicker.svelte';
  import TabBar from '$lib/ui/TabBar.svelte';
  import { modpackBrowseState } from './browse-state.svelte';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { MODPACKS_STEPS } from '$lib/onboarding/contextual-tours';
  import { t } from '$lib/i18n';

  // Top-level pane rendered inside the Modpacks modal. Owns the Browse |
  // Imported sub-tab shell (lazy-mount + CSS-hide so search / pagination /
  // filter state survive switching tabs), the pack-detail drawer, and the
  // inspect → picker step of an import. Once the user confirms the picker, the
  // actual import is handed up to the page via `onImport` — the PAGE owns the
  // op-queue store + OperationsView widget, so the modal can be closed
  // mid-import without losing progress or the new-instance handoff.
  //
  // The picker dialog stashes the file path on the `ModpackSummary` (`._path`)
  // so the confirm step can forward it without re-inspecting.

  let {
    instances,
    onInstanceCreated,
    onListChanged,
    onImport,
  }: {
    instances: InstanceWithStatus[];
    onInstanceCreated: (id: string) => void;
    onListChanged?: () => void;
    onImport?: (req: ModpackImportRequest) => void;
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
      filters: [{ name: $t('common.fileFilter.modpack'), extensions: ['mrpack', 'zip'] }],
    });
    if (typeof r === 'string') {
      resetHints();
      void inspect(r);
    }
  }

  // Picker / progress / drawer state machine.
  let summary = $state<ModpackSummary | null>(null);
  let error = $state<string | null>(null);
  let drawerHit = $state<ModpackHit | null>(null);
  // MC version filter the user had in the toolbar when they clicked the
  // card. Null = no filter → drawer lists every version of the pack.
  // Set = drawer only shows versions matching that MC, so the visible
  // list reflects the filtered grid the user came from.
  let drawerMcFilter = $state<string | null>(null);

  // Hint params for `modpack_import`. Set when the user lands here from
  // the Modrinth Browse flow (so the new instance gets `mrpack_project_id`
  // + `mrpack_source = 'modrinth'` stamped onto it without a second API
  // hop on the Rust side). Drag-drop imports keep these null and the
  // orchestrator falls back to the version-id auto-lookup added in P1.
  let hintProjectId = $state<string | null>(null);
  let hintSource = $state<ModSource | null>(null);
  // Modrinth version id of the picked version (Browse flow). Threaded to
  // `modpack_import` so the new instance stores `mrpack_version_id` — the
  // stable identifier the update flow compares against.
  let hintVersionId = $state<string | null>(null);

  async function inspect(path: string) {
    error = null;
    try {
      const r = await commands.modpackInspect(path);
      if (r.status === 'ok') {
        // Stash the path on the summary so confirmImport can use it
        // without re-prompting the user.
        summary = { ...r.data, _path: path } as ModpackSummary & { _path: string };
      } else {
        error = formatError(r.error);
      }
    } catch (e) {
      // A thrown/rejected invoke (e.g. a backend panic) must not vanish —
      // surface it so the user sees why the picker never opened.
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function resetHints() {
    hintProjectId = null;
    hintSource = null;
    hintVersionId = null;
  }

  // Per-card busy for the browse quick-install (resolve latest + download).
  // SvelteSet so two cards installing in parallel don't clobber each other.
  const quickInstalling = new SvelteSet<string>();

  // Quick-install = the detail modal's recommended-version install, surfaced on
  // the browse card. Resolve the latest version exactly like the modal (newest
  // visible version, honouring the active MC filter) and reuse the existing
  // fetch → inspect → ImportPickerDialog flow — nothing about the import is
  // reinvented, so the transparency review still runs. Any failure (no matching
  // version, network/decode error, or distribution blocked) falls back to
  // opening the detail modal, which surfaces the version list or the
  // "Open on CurseForge" body.
  async function quickInstall(hit: ModpackHit, mc: string | null) {
    const openModal = () => {
      drawerHit = hit;
      drawerMcFilter = mc;
    };
    if (hit.distribution_allowed === false) {
      openModal();
      return;
    }
    quickInstalling.add(hit.project_id);
    try {
      const v = await commands.modpackGetVersions(hit.source, hit.project_id);
      if (v.status !== 'ok') {
        openModal();
        return;
      }
      const visible = mc ? v.data.filter((x) => x.game_versions.includes(mc)) : v.data;
      // Backend returns versions newest-first; [0] is the recommended pick.
      const recommended = visible[0];
      if (!recommended) {
        openModal();
        return;
      }
      const r = await commands.modpackFetchToTemp(hit.source, hit.project_id, recommended.id);
      if (r.status === 'ok') {
        hintProjectId = hit.project_id;
        hintSource = hit.source;
        hintVersionId = recommended.id;
        await inspect(r.data);
      } else {
        // distribution disabled or any other fetch error → let the modal handle it.
        openModal();
      }
    } catch {
      openModal();
    } finally {
      quickInstalling.delete(hit.project_id);
    }
  }

  // The user confirmed the picker. Hand the import request up to the page
  // (which owns the op-queue store + OperationsView widget) and clear the
  // local picker state. Synchronous — ModpacksTab no longer awaits the import,
  // so closing the modal here is harmless.
  function confirmImport(selectedShas: string[]) {
    if (!summary) return;
    const path = (summary as ModpackSummary & { _path: string })._path;
    summary = null;
    onImport?.({
      path,
      selectedShas,
      projectId: hintProjectId,
      source: hintSource,
      versionId: hintVersionId,
    });
    resetHints();
  }
</script>

<div class="flex flex-col h-full">
  <!-- Second level: Browse | Imported sub-tabs, with the Source context switch
       pinned right — mirrors the Add-ons tab's sub-tab row. Source is a context
       switch (which catalogue), not a narrowing filter, so it lives here rather
       than in the filter toolbar below. -->
  <div
    class="border-b flex items-center justify-between gap-1 px-3 bg-surface"
    data-tour-ctx="modpacks-tabs"
  >
    <TabBar
      tabs={[
        { id: 'browse', label: $t('modpacks.tab.browse') },
        { id: 'imported', label: $t('modpacks.tab.imported') },
      ]}
      active={activeSub}
      onChange={(id) => (activeSub = id as SubTab)}
    />
    <SourcePicker
      value={modpackBrowseState.source}
      allowFtb={true}
      allowAtlauncher={true}
      onChange={(v) => (modpackBrowseState.source = v)}
    />
  </div>

  <div class="px-4 pt-3" data-tour-ctx="modpacks-dropzone">
    <FileDropzone label={$t('modpacks.tab.dropzoneLabel')} onClick={importFromFile} />
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
          onQuickInstall={quickInstall}
          installingIds={quickInstalling}
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
