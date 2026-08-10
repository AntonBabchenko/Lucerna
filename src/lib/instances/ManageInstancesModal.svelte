<script lang="ts">
  import { get } from 'svelte/store';
  import {
    commands,
    type InstanceWithStatus,
    type LoaderKind,
    type VersionEntry,
    type Error as IpcError,
    type MemoryBounds,
    type ModLocalCompat,
  } from '$lib/ipc/bindings';
  import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';
  import InstanceAvatarEdit from '$lib/instances/InstanceAvatarEdit.svelte';
  import InstanceFolderRow from '$lib/instances/InstanceFolderRow.svelte';
  import IntegritySection from '$lib/instances/IntegritySection.svelte';
  import { displayLauncher } from '$lib/instances/launcher-display';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { shouldFocusField, type ManageFocusField } from '$lib/instances/manage-focus';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { loaderOutcomeToast, compatSummaryFromScan } from '$lib/instances/integrity-messages';
  import { compatScanEntries, ensureCompatScan } from '$lib/mods/compat-scan.svelte';
  import { formatHeapLabel } from '$lib/instances/heap';
  import { FALLBACK_MEMORY_BOUNDS, loadMemoryBounds } from '$lib/instances/memory-bounds';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { formatError } from '$lib/ipc/format-error';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { MANAGE_STEPS } from '$lib/onboarding/contextual-tours';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { fieldFlash } from '$lib/ui/field-flash';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import SplitterHandle from '$lib/ui/SplitterHandle.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { clampPanelWidth } from '$lib/ui/splitter';
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';
  import { tooltip } from '$lib/ui/tooltip';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { dataLocation } from '$lib/settings/data-location.svelte';
  import { dataRootCreateDisabledKey } from '$lib/settings/data-root-gating';

  let {
    open = $bindable(),
    instances = $bindable<InstanceWithStatus[]>(),
    activeInstance = $bindable<InstanceWithStatus | null>(),
    versions,
    onChanged,
    isRunning = false,
    initialSelectedId = null,
    focusField = null,
    onCloneRequest = () => {},
    onShortcutRequest,
    onTranslationsRequest,
  }: {
    open: boolean;
    instances: InstanceWithStatus[];
    activeInstance: InstanceWithStatus | null;
    versions: VersionEntry[];
    onChanged: () => void;
    isRunning?: boolean;
    // Open the clone dialog for this instance (hosted by the page so the
    // sidebar entry point shares it). Defaults to a no-op for bare mounts.
    onCloneRequest?: (instanceId: string) => void;
    /** Undefined on platforms without desktop-shortcut support — the button is
     *  then omitted rather than shown disabled. */
    onShortcutRequest?: (instanceId: string) => void;
    /** Open the per-instance translation editor for this instance (hosted by
     *  the page, which owns the single LocalizationModal mount). Undefined on
     *  bare mounts — the button is then omitted. */
    onTranslationsRequest?: (id: string) => void;
    // When set (opened via a per-row "manage this profile" action), seed the
    // detail selection from THIS id rather than the active instance. Switching
    // the active instance is async (an IPC round-trip), so at open time
    // `activeInstance` may still be the previously-active one — seeding from the
    // explicitly-clicked id avoids opening Manage on the wrong profile.
    initialSelectedId?: string | null;
    /** Which control to scroll to and flash when the modal opens. Set by the
     *  Overview card the user clicked; null opens the modal unchanged. */
    focusField?: ManageFocusField | null;
  } = $props();

  let selectedId = $state<string | null>(null);
  let selected = $derived(instances.find((i) => i.id === selectedId) ?? null);
  let createMode = $state(false);

  // Local name filter for the sidebar list — only surfaces once the list is
  // long enough that scanning becomes a chore. Display-only: filtering never
  // changes the selection, so the detail panel keeps showing the selected
  // instance even if it is hidden from the list.
  const FILTER_THRESHOLD = 8;
  // Draggable list/detail split. Not persisted — reopening starts from the
  // default, same as the skin editor's panel.
  //
  // The floor is a constant because it is about a name staying readable, not
  // about available space. The ceiling is DERIVED: the list may grow until the
  // form would drop below its comfortable width. LIST_FALLBACK_MAX only applies
  // before the first measurement (or where ResizeObserver is absent) and is the
  // fixed ceiling this modal shipped with.
  const LIST_MIN_WIDTH = 180;
  const LIST_FALLBACK_MAX = 420;
  const DETAIL_MIN_WIDTH = 720;
  // Pane width at which the detail form has room for two columns.
  const TWO_COLUMN_AT = 1100;
  let listWidth = $state(220);
  let rowWidth = $state(0);
  const listMax = $derived(
    rowWidth > 0 ? Math.max(LIST_MIN_WIDTH, rowWidth - DETAIL_MIN_WIDTH) : LIST_FALLBACK_MAX,
  );
  // Pane width = row minus the list and the handle. The threshold is the PANE,
  // not the window, so dragging the splitter right can legitimately collapse
  // the columns — there really is less room for them.
  const twoColumn = $derived(rowWidth > 0 && rowWidth - listWidth - 4 >= TWO_COLUMN_AT);

  // Owned here rather than by SplitterHandle: the shared handle takes bounds as
  // props and stays observer-free, which also keeps it safe to render in the
  // component tests. Mirrors how SkinEditorModal feeds it a derived max.
  function observeRow(node: HTMLElement) {
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      rowWidth = node.clientWidth;
      // Re-clamp when the window shrinks the ceiling below the current width.
      listWidth = clampPanelWidth(listWidth, LIST_MIN_WIDTH, listMax);
    });
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
      },
    };
  }
  // Mirrors MAX_INSTANCE_NAME_LEN in src-tauri/src/commands/mod.rs — the backend
  // is the source of truth and rejects longer names; this only drives the input
  // maxlength + counter so the UI agrees with the validator.
  const NAME_MAX = 32;
  let filterQuery = $state('');
  let filteredInstances = $derived(
    filterQuery.trim()
      ? instances.filter((i) => i.name.toLowerCase().includes(filterQuery.trim().toLowerCase()))
      : instances,
  );

  // When the modal opens, default the selection to the currently-active
  // instance (the one the user is playing on the main view). Otherwise
  // the detail panel either shows empty state (selectedId=null) or
  // whatever was selected in a previous session — both surprising. The
  // user expects to manage the instance they just had open.
  $effect(() => {
    if (open && selectedId === null) {
      selectedId = initialSelectedId ?? activeInstance?.id ?? instances[0]?.id ?? null;
    }
  });

  let modalError = $state<string | null>(null);
  let deleteConfirmOpen = $state(false);
  // Deleting an instance destroys ALL of its worlds (plus mods/configs), so the
  // gate is at least as strong as the single-world delete: the user must type
  // the literal word "Delete" to confirm. Mirrors DeleteWorldDialog.
  const DELETE_CONFIRM_WORD = 'Delete';
  let deleteConfirmTyped = $state('');
  const canConfirmDelete = $derived(deleteConfirmTyped === DELETE_CONFIRM_WORD);

  // Transient per-field "Saved" confirmation. The detail editor auto-saves each
  // field independently (no Save button), so the user otherwise gets no signal a
  // blur/drag actually persisted. Track the most-recently-saved field; clear it
  // after a short delay or when the user re-edits that field.
  const SAVED_HINT_MS = 1500;
  type SavedField = 'name' | 'memory' | 'jvm';
  let savedField = $state<SavedField | null>(null);
  let savedTimer: ReturnType<typeof setTimeout> | null = null;
  function markSaved(f: SavedField) {
    savedField = f;
    if (savedTimer) clearTimeout(savedTimer);
    savedTimer = setTimeout(() => {
      if (savedField === f) savedField = null;
    }, SAVED_HINT_MS);
  }
  function clearSaved(f: SavedField) {
    if (savedField === f) savedField = null;
  }
  // Clear any pending "Saved" timer when the modal unmounts.
  $effect(() => () => {
    if (savedTimer) clearTimeout(savedTimer);
  });

  // Mod-compat summary for the current instance after an MC/loader change.
  // Sourced from the shared offline scan (`compat-scan.svelte`), not a
  // platform query: the platform answers "does this PROJECT have a build for
  // (mc, loader)?", which every mod in the original bug report could answer
  // yes to even though the FILE on disk was still the old build.
  let compatRows = $state<ModLocalCompat[] | null>(null);
  // Set when the compat scan could not be computed. `ensureCompatScan`
  // absorbs an ordinary backend error itself (keeps the previous scan in
  // place, by design — see compat-scan.svelte's doc comment), so this can
  // only still be reached by a genuine transport-level throw. Rare, but real
  // enough to keep the quiet "couldn't check" note rather than going silent.
  let compatCheckFailed = $state(false);
  // Reset compat state when the selected instance changes.
  $effect(() => {
    void selectedId;
    compatRows = null;
    compatCheckFailed = false;
    savedField = null;
  });

  // The error region scrolls into view when an error first appears, so a failure
  // from a control above the fold isn't missed at the bottom of the panel. Gate
  // on the null→message transition (non-reactive cursor, like lastNameSyncId) so
  // a second failure on an already-visible region doesn't yank the viewport.
  let errorRegionEl = $state<HTMLElement | null>(null);
  let lastScrolledError: string | null = null;
  $effect(() => {
    if (modalError && !lastScrolledError && errorRegionEl) {
      errorRegionEl.scrollIntoView?.({ block: 'nearest' });
    }
    lastScrolledError = modalError;
  });

  // Pending pack-detach confirm state.
  // Holds { kind: 'mc', value: string } or { kind: 'loader', loaderKind, loaderVersion }
  // when awaiting the user's decision.
  type PendingChange =
    | { kind: 'mc'; value: string }
    | { kind: 'loader'; loaderKind: LoaderKind; loaderVersion: string | null };
  let pendingChange = $state<PendingChange | null>(null);

  // Snapshot toggle for the MC version pickers. Off by default — most users
  // want stable releases. Deliberately shared across the create form and the
  // detail editor: the cross-flip is invisible in practice because `createMode`
  // gates which of the two checkboxes renders (they never show at once), and a
  // user who wants snapshots usually wants them in both contexts.
  let showSnapshots = $state(false);
  let visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );

  // Options for the MC-version <Select>s. Keep the empty "Choose…" row so it is
  // re-selectable and so Select greys the trigger when no version is picked.
  const mcVersionOptions = $derived([
    { value: '', label: $t('instance.manage.chooseMcOption') },
    ...visibleVersions.map((v) => ({ value: v.id, label: v.id })),
  ]);

  // Create form state.
  let draftName = $state('');
  let draftMc = $state('');
  let draftLoader = $state<LoaderKind>('vanilla');
  let draftLoaderVersion = $state<string | null>(null);
  // Guards a double-clicked Create from spawning two profiles (the disabled
  // reason stays satisfied across the await, so only a busy flag stops re-entry).
  let createPending = $state(false);

  // True when an in-flight command's target instance is no longer the live
  // selection (switched away) or the modal closed — used to no-op stale
  // completions so a previous instance's result never lands on the current one.
  function isStale(id: string) {
    return !open || selectedId !== id;
  }

  // Detail form state — reactive to `selected`.
  let nameDraft = $state('');
  // Resync the editable name only when the SELECTED INSTANCE changes, not on
  // every `selected` object-identity churn. A background refreshInstances()
  // (game exit, integrity/import completion) replaces the whole `instances`
  // array with the same selectedId; gating on the id keeps an in-progress edit
  // from being silently clobbered. Switching to another instance still resyncs.
  let lastNameSyncId: string | null = null;
  $effect(() => {
    if (selected && selected.id !== lastNameSyncId) {
      nameDraft = selected.name;
      lastNameSyncId = selected.id;
    }
  });

  // Local heap draft so dragging the slider updates the label live WITHOUT a
  // disk write per tick: onInput updates the draft, and we persist once on
  // release via MemorySlider's onCommit. Seeded id-gated like the name draft so
  // a background refresh doesn't reset an active drag. (MemorySlider owns the
  // thumb-tracking fix and adaptive bounds, so no imperative re-apply here.)
  let heapDraft = $state(0);
  let lastHeapSyncId: string | null = null;
  $effect(() => {
    if (selected && selected.id !== lastHeapSyncId) {
      heapDraft = selected.max_heap_mb;
      lastHeapSyncId = selected.id;
    }
  });

  // Create-form heap draft. Deliberately NOT `heapDraft` above: that one belongs
  // to the detail editor and is reseeded id-gated on every background refresh,
  // which would stomp a heap the user picked mid-create. `null` = untouched, so
  // the effective value tracks the adaptive default until the user actually
  // drags — which also sidesteps the race where the bounds resolve only after
  // the form is already open.
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

  // Advanced: optional initial heap (-Xms). null = unset (JVM default). Seeded
  // id-gated like the other drafts so a background refresh doesn't clobber it.
  let minHeapDraft = $state<number | null>(null);
  let lastMinHeapSyncId: string | null = null;
  $effect(() => {
    if (selected && selected.id !== lastMinHeapSyncId) {
      minHeapDraft = selected.min_heap_mb;
      lastMinHeapSyncId = selected.id;
    }
  });

  // Auto-clear stale modalError when the user navigates away from
  // whatever caused it — switching instances, opening/closing the
  // create form, or picking a different MC/loader in create draft.
  // Without this, a "quilt has no version for 26.1.2" error from a
  // previous attempt would linger on top of an unrelated screen.
  let createDisabledReason = $derived.by(() => {
    if (!createMode) return '';
    if (!draftName.trim()) return get(t)('instance.error.nameRequired');
    if (!draftMc) return get(t)('instance.error.pickMcFirst');
    if (draftLoader !== 'vanilla' && !draftLoaderVersion)
      return get(t)('instance.error.loaderNoSupport', {
        loader: displayLoader(draftLoader),
        mc: draftMc,
      });
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

  // §7 fallback gating: the data root is unavailable, so creating a new
  // instance would write it into the wrong (temporary default) root. Gates
  // the "New instance" entry point itself — separate from createDisabledReason
  // above, which validates the in-progress create FORM once it's open.
  const dataRootBlockedReason = $derived.by(() => {
    const key = dataRootCreateDisabledKey(dataLocation.fellBack);
    return key === null ? null : get(t)(key);
  });

  function ipcErrorMessage(e: IpcError): string {
    // Modal-local shorter wording for the two name-validation cases (the
    // modal context makes "Instance" redundant). Everything else
    // delegates to the shared formatError so no IPC variant leaks raw
    // JSON if e.g. openInstanceFolder returns Error::Io.
    if (e.kind === 'instance_name_empty') return get(t)('instance.error.nameEmpty');
    if (e.kind === 'instance_name_too_long')
      return get(t)('instance.error.nameTooLong', { actual: e.actual, max: e.max });
    // Folder-rename cases. Same reasoning as above: inside this modal the
    // "Instance" prefix the shared formatter adds is redundant.
    if (e.kind === 'instance_dir_name_empty') return get(t)('instance.error.dirNameEmpty');
    if (e.kind === 'instance_dir_name_taken')
      return get(t)('instance.error.dirNameTaken', { name: e.name });
    if (e.kind === 'instance_dir_name_reserved')
      return get(t)('instance.error.dirNameReserved', { name: e.name });
    if (e.kind === 'instance_dir_locked')
      return get(t)('instance.error.dirLocked', { name: e.name });
    if (e.kind === 'path_not_launchable') return get(t)('instance.error.pathNotLaunchable');
    return formatError(e);
  }

  function openCreate() {
    createMode = true;
    draftName = '';
    draftMc = '';
    draftLoader = 'vanilla';
    draftLoaderVersion = null;
    createHeapDraft = null;
    modalError = null;
    filterQuery = '';
  }

  async function submitCreate() {
    // Re-entry guard for a rapid double-click (validation runs first so a
    // validation early-return never strands the busy flag).
    if (createPending) return;
    // Belt-and-braces: the entry point is already disabled via
    // dataRootBlockedReason, but createMode can be entered before a late
    // fell_back flip lands, so guard the actual mutation too.
    if (dataLocation.fellBack) return;
    if (!draftName.trim()) {
      modalError = get(t)('instance.error.nameRequired');
      return;
    }
    if (draftLoader !== 'vanilla' && !draftMc) {
      modalError = get(t)('instance.error.pickMcFirst');
      return;
    }
    if (draftLoader !== 'vanilla' && !draftLoaderVersion) {
      // Belt-and-braces: the Create button is also disabled in this
      // state via createDisabledReason. This branch catches the
      // in-flight race where load() hasn't resolved yet.
      modalError = get(t)('instance.error.loaderNoSupport', {
        loader: displayLoader(draftLoader),
        mc: draftMc,
      });
      return;
    }
    createPending = true;
    try {
      const result = await commands.createInstance(
        draftName.trim(),
        draftMc,
        draftLoader,
        draftLoaderVersion,
        createHeapMb,
      );
      if (result.status === 'ok') {
        createMode = false;
        // Make the newly created instance active — matches user intent
        // ("I just made this thing to play it"). Editing an existing
        // non-active instance still leaves the active unchanged.
        await commands.setActiveInstance(result.data.id);
        onChanged();
        selectedId = result.data.id;
        // Creating a profile silently switches the active one — say so.
        pushSuccess(get(t)('instance.manage.createdActiveToast', { name: result.data.name }));
      } else {
        modalError = ipcErrorMessage(result.error);
      }
    } finally {
      createPending = false;
    }
  }

  async function commitName() {
    if (!selected || nameDraft === selected.name) return;
    if (!nameDraft.trim()) {
      // Empty/whitespace: snap the field back to the saved name rather than
      // leaving a blank box (an accidental clear reads as a silent no-op).
      nameDraft = selected.name;
      return;
    }
    const id = selected.id;
    const result = await commands.setInstanceName(id, nameDraft.trim());
    if (isStale(id)) return;
    if (result.status === 'ok') {
      onChanged();
      markSaved('name');
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  // Takes the NEW mc/loader explicitly rather than reading `selected`: after a
  // change, `onChanged()` refreshes the `instances` prop via an async round-trip
  // that isn't awaited here, so `selected` still holds the OLD config. Querying
  // compatibility against stale values would defeat the whole point of the
  // summary. The fresh values come straight from the command result.
  async function runModCompatCheck(id: string, mc: string, loader: LoaderKind) {
    try {
      // force: true — the mod set didn't change the scan key, so an unforced
      // call would be deduplicated away against a scan from before this
      // MC/loader change.
      await ensureCompatScan(id, mc, loader, { force: true });
      if (isStale(id)) return;
      compatRows = compatScanEntries();
      compatCheckFailed = false;
    } catch {
      // ensureCompatScan absorbs an ordinary backend error and keeps the
      // previous scan in place; reaching here means the IPC call itself threw.
      if (isStale(id)) return;
      compatRows = null;
      compatCheckFailed = true;
    }
  }

  // `id` is captured by the caller before any await so a mid-flight selection
  // switch can't redirect this change's side effects onto another instance.
  async function applyMcChange(id: string, mc: string) {
    const result = await commands.changeInstanceMc(id, mc);
    if (isStale(id)) return;
    if (result.status === 'ok') {
      const toast = loaderOutcomeToast(result.data.loader_outcome, mc);
      if (toast?.kind === 'success') pushSuccess(toast.text);
      else if (toast?.kind === 'warning') pushWarning(toast.text);
      onChanged();
      const fresh = result.data.instance;
      await runModCompatCheck(fresh.id, fresh.mc_version, fresh.loader);
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
    await applyMcChange(selected.id, mc);
  }

  async function applyLoaderChange(id: string, kind: LoaderKind, version: string | null) {
    const result = await commands.setInstanceLoader(id, kind, version);
    if (isStale(id)) return;
    if (result.status === 'ok') {
      onChanged();
      await runModCompatCheck(result.data.id, result.data.mc_version, result.data.loader);
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  async function commitLoader(kind: LoaderKind, version: string | null) {
    if (!selected) return;
    // Validate up front (selected is fresh here, pre-await) so a missing MC
    // version fails fast instead of after the detach prompt.
    if (kind !== 'vanilla' && !selected.mc_version) {
      modalError = get(t)('instance.error.pickMcFirst');
      return;
    }
    if (selected.mrpack_name) {
      pendingChange = { kind: 'loader', loaderKind: kind, loaderVersion: version };
      return;
    }
    await applyLoaderChange(selected.id, kind, version);
  }

  async function confirmDetachAndContinue() {
    if (!selected || !pendingChange) return;
    const id = selected.id;
    const change = pendingChange;
    pendingChange = null;
    const detachResult = await commands.detachInstancePack(id);
    if (isStale(id)) return;
    if (detachResult.status === 'error') {
      modalError = ipcErrorMessage(detachResult.error);
      return;
    }
    // After detach onChanged() refreshes the instance list; but we need to
    // apply the change against the updated selected. Trigger the change directly.
    onChanged();
    if (change.kind === 'mc') {
      await applyMcChange(id, change.value);
    } else {
      await applyLoaderChange(id, change.loaderKind, change.loaderVersion);
    }
  }

  async function keepAndContinue() {
    if (!selected || !pendingChange) return;
    const id = selected.id;
    const change = pendingChange;
    pendingChange = null;
    if (change.kind === 'mc') {
      await applyMcChange(id, change.value);
    } else {
      await applyLoaderChange(id, change.loaderKind, change.loaderVersion);
    }
  }

  function cancelPending() {
    pendingChange = null;
  }

  async function setMemory(mb: number) {
    if (!selected) return;
    const id = selected.id;
    const result = await commands.setInstanceMemory(id, mb);
    if (isStale(id)) return;
    if (result.status === 'ok') {
      onChanged();
      markSaved('memory');
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  async function setJvmArgs(args: string) {
    if (!selected) return;
    const id = selected.id;
    const result = await commands.setInstanceJvmArgs(id, args);
    if (isStale(id)) return;
    if (result.status === 'ok') {
      onChanged();
      markSaved('jvm');
    } else {
      modalError = ipcErrorMessage(result.error);
    }
  }

  async function commitMinHeap() {
    if (!selected) return;
    const id = selected.id;
    // Empty/0 clears it (None). Cap at the current max heap — an Xms larger than
    // Xmx makes the JVM refuse to start.
    const raw = minHeapDraft && minHeapDraft > 0 ? minHeapDraft : null;
    const value = raw === null ? null : Math.min(raw, heapDraft);
    minHeapDraft = value; // reflect the clamp/clear back into the field
    if (value === selected.min_heap_mb) return; // unchanged — no write
    const result = await commands.setInstanceMinHeap(id, value);
    if (isStale(id)) return;
    if (result.status === 'ok') onChanged();
    else modalError = ipcErrorMessage(result.error);
  }

  async function openFolder() {
    if (!selected) return;
    const result = await commands.openInstanceFolder(selected.id);
    // Folder-open is not tied to a visible field, so a toast is the right
    // channel (ToastHost is aria-live=polite) rather than the inline alert.
    if (result.status === 'error')
      pushWarning(get(t)('instance.manage.openFolderFailed'), [ipcErrorMessage(result.error)]);
  }

  async function openSourceFolder() {
    if (!selected?.imported_from) return;
    const result = await commands.openImportedSourceFolder(selected.id);
    if (result.status === 'error')
      pushWarning(get(t)('instance.manage.openSourceFolderFailed'), [
        ipcErrorMessage(result.error),
      ]);
  }

  async function deleteSelected() {
    if (!selected) return;
    if (instances.length <= 1) return; // belt-and-braces; the button is also disabled
    const result = await commands.deleteInstance(selected.id);
    if (result.status === 'ok') {
      selectedId = null;
      lastNameSyncId = null;
      lastHeapSyncId = null;
      lastMinHeapSyncId = null;
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
    // Also clear the name-resync cursor so reopening on the same instance
    // re-seeds nameDraft from the saved name (an uncommitted edit is discarded
    // on close, not resurrected on reopen).
    lastNameSyncId = null;
    lastHeapSyncId = null;
    lastMinHeapSyncId = null;
    filterQuery = '';
    savedField = null;
  }
</script>

{#snippet activeChip()}
  <span
    class="shrink-0 rounded-full bg-accent px-1.5 py-0.5 text-[10px] font-semibold leading-none text-white"
  >
    {$t('instance.manage.activeBadge')}
  </span>
{/snippet}

{#snippet savedBadge(field: SavedField)}
  {#if savedField === field}
    <span class="inline-flex items-center gap-1 font-normal text-success">
      <Icon name="success" size={12} />
      {$t('instance.manage.saved')}
    </span>
  {/if}
{/snippet}

{#snippet noVersionsNotice()}
  {#if visibleVersions.length === 0}
    <p class="mb-3 text-xs text-warning-text">
      {versions.length === 0
        ? $t('instance.manage.noVersions')
        : $t('instance.manage.noReleasesEnableSnapshots')}
    </p>
  {/if}
{/snippet}

{#if open}
  <Modal
    ariaLabelledby="manage-instances-title"
    onClose={close}
    panelClass="w-full h-full overflow-hidden flex flex-col"
  >
    <header class="flex items-center justify-between px-4 py-2 border-b">
      <h2 id="manage-instances-title" class="font-semibold text-primary">
        {$t('instance.manage.title')}
      </h2>
      <CloseButton onClick={close} ariaLabel={$t('instance.manage.closeLabel')} />
    </header>
    <div class="flex flex-1 overflow-hidden" use:observeRow>
      <aside
        class="shrink-0 p-2 flex flex-col gap-2"
        style="width:{listWidth}px"
        data-tour-ctx="manage-list"
        aria-label={$t('instance.manage.listRegionLabel')}
      >
        {#if dataRootBlockedReason}
          <span
            class="inline-flex shrink-0 w-full"
            use:tooltip={{ text: dataRootBlockedReason, describe: false }}
          >
            <button type="button" class="btn-primary btn-sm w-full" disabled>
              {$t('instance.manage.newInstanceBtn')}
            </button>
          </span>
        {:else}
          <button type="button" class="shrink-0 btn-primary btn-sm w-full" onclick={openCreate}>
            {$t('instance.manage.newInstanceBtn')}
          </button>
        {/if}
        {#if instances.length > FILTER_THRESHOLD}
          <input
            type="text"
            class="shrink-0 border rounded px-2 py-1 text-sm"
            placeholder={$t('instance.manage.filterPlaceholder')}
            aria-label={$t('instance.manage.filterPlaceholder')}
            bind:value={filterQuery}
          />
        {/if}
        <div class="flex-1 overflow-y-auto flex flex-col gap-1">
          {#each filteredInstances as i (i.id)}
            <button
              class="text-left px-2 py-1 rounded text-sm hover:bg-subtle"
              class:bg-accent-soft={i.id === selectedId}
              aria-current={i.id === selectedId}
              onclick={() => {
                createMode = false;
                selectedId = i.id;
              }}
            >
              <div class="font-medium flex items-center gap-1.5">
                <!-- Same 20px avatar the sidebar rows use, so an instance looks
                     the same in both lists. The ready/download glyph stays: it
                     carries the install status, not identity. -->
                <InstanceAvatar instance={i} size={20} />
                <Icon
                  name={i.ready ? 'success' : 'download'}
                  class="shrink-0"
                  label={i.ready
                    ? $t('instance.manage.iconReady')
                    : $t('instance.manage.iconDownloadNeeded')}
                />
                <span
                  class="truncate min-w-0 flex-1"
                  use:tooltip={{ text: i.name, whenOverflowing: true }}>{i.name}</span
                >
                {#if i.integrity && !i.integrity.healthy}
                  <!-- The span carries the hover tooltip (title); the icon
                       carries the accessible name (label → role="img" +
                       aria-label), so pointer and screen-reader users get
                       the same "N problems" text. -->
                  <span
                    class="inline-flex shrink-0 text-warning-text"
                    use:tooltip={$t('instance.integrity.statusProblems', {
                      count: i.integrity.problem_count,
                    })}
                  >
                    <Icon
                      name="warning"
                      label={$t('instance.integrity.statusProblems', {
                        count: i.integrity.problem_count,
                      })}
                    />
                  </span>
                {/if}
                {#if i.id === activeInstance?.id}
                  {@render activeChip()}
                {/if}
              </div>
              <div class="text-xs text-muted truncate">
                {displayLoader(i.loader)} · {i.mc_version || $t('instance.manage.pickMc')}
              </div>
            </button>
          {/each}
          {#if filterQuery.trim() && filteredInstances.length === 0}
            <p class="text-xs text-muted px-2 py-1">{$t('instance.manage.filterNoMatches')}</p>
          {/if}
        </div>
      </aside>
      <!-- The list sits before the handle, so dragging right widens it. -->
      <SplitterHandle
        bind:width={listWidth}
        min={LIST_MIN_WIDTH}
        max={listMax}
        label={$t('instance.manage.resizeList')}
        testId="manage-list-splitter"
      />
      <!-- Column, not a plain scroller: the body scrolls and the action row
           stays pinned. In a full-window modal a flow-positioned action row
           would sit far below the fold, so Close would need a scroll. -->
      <section class="flex flex-1 min-w-0 flex-col overflow-hidden" data-tour-ctx="manage-form">
        <!-- Form content is capped: a name input stretched across a maximised
             window is unreadable. The cap belongs to the content, not the pane,
             so the pinned action row still spans the full width. -->
        <div class="flex-1 overflow-y-auto p-4">
          <div class={twoColumn && !createMode ? 'w-full' : 'mx-auto max-w-[720px]'}>
            {#if createMode}
              <h3 class="font-semibold text-primary mb-3">{$t('instance.manage.createHeading')}</h3>
              <label for="create-name" class="mb-1 flex justify-between text-xs text-secondary">
                <span>{$t('instance.manage.nameLabel')}</span>
                <span class="text-placeholder font-normal"
                  >{$t('instance.manage.nameCounter', {
                    count: draftName.length,
                    max: NAME_MAX,
                  })}</span
                >
              </label>
              <input
                id="create-name"
                class="border rounded px-2 py-1 w-full mb-3"
                maxlength={NAME_MAX}
                bind:value={draftName}
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
              {@render noVersionsNotice()}

              <LoaderPicker
                mc={draftMc}
                bind:loader={draftLoader}
                bind:loaderVersion={draftLoaderVersion}
              />

              <label for="create-memory" class="mt-3 mb-1 block text-xs text-secondary">
                {$t('instance.manage.memoryLabel', { value: formatHeapLabel(createHeapMb) })}
              </label>
              <!-- No onCommit: this is a draft. Unlike the detail editor, which
                   persists on release, the value is written once by Create. -->
              <MemorySlider
                id="create-memory"
                class="mb-1"
                warnClass="mb-3"
                reserveWarnSpace
                valueMb={createHeapMb}
                onInput={(mb) => (createHeapDraft = mb)}
              />

              <div class="flex items-center justify-end gap-2 mt-4">
                <!-- Visible reason instead of a tooltip on a disabled button: the
                 button stays keyboard-reachable and submitCreate surfaces the
                 same reason as an announced modalError on click. -->
                {#if createDisabledReason}
                  <span class="mr-auto text-xs text-secondary">{createDisabledReason}</span>
                {/if}
                <button
                  type="button"
                  class="btn-secondary btn-sm"
                  onclick={() => (createMode = false)}
                >
                  {$t('instance.manage.cancelBtn')}
                </button>
                <BusyButton class="btn-primary btn-sm" busy={createPending} onclick={submitCreate}>
                  {$t('instance.manage.createBtn')}
                </BusyButton>
              </div>
            {:else if selected}
              <!-- Two groups, split by meaning rather than field count: what you
                   SET on the left, what you INSPECT on the right. Below
                   TWO_COLUMN_AT they stack, and the order within each group is
                   unchanged so the manage-form tour still tracks. -->
              <div class={twoColumn ? 'grid grid-cols-2 items-start gap-x-8' : ''}>
                <div>
                  <!-- Identity row: the picture plus the active badge. The instance
               name is NOT repeated here — it lives in the Name field below,
               and printing it twice only crowded the top of the pane. -->
                  <div class="mb-3 flex items-center gap-3">
                    <InstanceAvatarEdit
                      instance={selected}
                      size={52}
                      testId="manage-avatar"
                      removeTestId="manage-avatar-remove"
                    />
                    {#if selected.id === activeInstance?.id}{@render activeChip()}{/if}
                  </div>

                  <label for="detail-name" class="mb-1 flex justify-between text-xs text-secondary">
                    <span>{$t('instance.manage.nameLabel')}</span>
                    <span class="flex items-center gap-2">
                      {@render savedBadge('name')}
                      <span class="text-placeholder font-normal"
                        >{$t('instance.manage.nameCounter', {
                          count: nameDraft.length,
                          max: NAME_MAX,
                        })}</span
                      >
                    </span>
                  </label>
                  <input
                    id="detail-name"
                    class="border rounded px-2 py-1 w-full mb-3"
                    maxlength={NAME_MAX}
                    bind:value={nameDraft}
                    oninput={() => clearSaved('name')}
                    onblur={commitName}
                  />

                  <InstanceFolderRow
                    instance={selected}
                    formatIpcError={ipcErrorMessage}
                    onRenamed={(updated) => {
                      // The directory name IS the id, so a rename changes it.
                      // Without following it here, `selected` (derived by matching
                      // `selectedId`) goes null and the detail pane empties the
                      // instant the repair succeeds — alarming at exactly the
                      // wrong moment. Pinned by "follows the new id" in
                      // manage-modal-tier1.test.ts.
                      selectedId = updated.id;
                      onChanged();
                    }}
                  />

                  <!-- The flash zone spans label + control + the snapshots toggle
               because they read as one field; the no-versions notice stays
               outside so an empty-list warning is never ringed as "the thing
               you asked for". -->
                  <div
                    data-focus-field="mc"
                    use:fieldFlash={{
                      active: focusField === 'mc',
                      focus: shouldFocusField('mc'),
                    }}
                  >
                    <label for="detail-mc-version" class="block text-xs text-secondary mb-1"
                      >{$t('instance.manage.mcVersionLabel')}</label
                    >
                    <span
                      class="block mb-1"
                      use:tooltip={{
                        text: isRunning ? $t('instance.manage.runningBlocked') : '',
                        describe: false,
                      }}
                    >
                      <Select
                        id="detail-mc-version"
                        class="w-full"
                        value={selected.mc_version}
                        options={mcVersionOptions}
                        disabled={isRunning}
                        onChange={(v) => setMc(String(v))}
                      />
                    </span>
                    <label class="text-xs flex items-center gap-1 mb-3">
                      <input type="checkbox" bind:checked={showSnapshots} />
                      {$t('instance.manage.showSnapshots')}
                    </label>
                  </div>
                  {@render noVersionsNotice()}

                  <!--
              Keyed on the instance id so the picker REMOUNTS when the user
              switches the selected instance. LoaderPicker tracks the previous
              loader in a non-reactive `prevLoader` to tell a user-driven loader
              switch from a mount/MC tweak; without a remount that value leaks
              across instances, so swapping to a modpack instance was mis-read as
              a loader change and falsely raised the pack-detach prompt.
            -->
                  <div
                    data-focus-field="loader"
                    use:fieldFlash={{
                      active: focusField === 'loader',
                      focus: shouldFocusField('loader'),
                    }}
                  >
                    <span
                      class="block"
                      use:tooltip={{
                        text: isRunning ? $t('instance.manage.runningBlocked') : '',
                        describe: false,
                      }}
                    >
                      {#key selected.id}
                        <LoaderPicker
                          mc={selected.mc_version}
                          loader={selected.loader}
                          loaderVersion={selected.loader_version}
                          disabled={isRunning}
                          onchange={async (l, v) => {
                            if (l !== selected!.loader || v !== selected!.loader_version) {
                              await commitLoader(l, v);
                            }
                          }}
                        />
                      {/key}
                    </span>
                  </div>

                  <!-- Compat summary + check-failure note share a polite live region so
               screen readers hear the advisory; both are empty in the idle state. -->
                  <StatusMessage
                    tone="warning"
                    live="polite"
                    withIcon
                    message={compatRows !== null
                      ? compatSummaryFromScan(compatRows, compatRows.length)
                      : null}
                    class="bg-warning-bg border border-warning-text/30 rounded px-2 py-1.5 mt-2 mb-1"
                  />
                  <StatusMessage
                    tone="info"
                    live="polite"
                    message={compatCheckFailed
                      ? $t('instance.manage.compatCheckUnavailable')
                      : null}
                    class="mt-2 mb-1"
                  />

                  <div
                    data-focus-field="memory"
                    use:fieldFlash={{
                      active: focusField === 'memory',
                      focus: shouldFocusField('memory'),
                    }}
                  >
                    <label
                      for="detail-memory"
                      class="mb-1 flex items-center justify-between text-xs text-secondary"
                    >
                      <span>
                        {$t('instance.manage.memoryLabel', {
                          value: formatHeapLabel(heapDraft),
                        })}
                      </span>
                      {@render savedBadge('memory')}
                    </label>
                    <MemorySlider
                      id="detail-memory"
                      class="mb-1"
                      warnClass="mb-3"
                      reserveWarnSpace
                      valueMb={heapDraft}
                      onInput={(mb) => {
                        heapDraft = mb;
                        clearSaved('memory');
                      }}
                      onCommit={(mb) => setMemory(mb)}
                    />
                  </div>
                </div>

                <div>
                  <details class="mb-3">
                    <summary class="cursor-pointer select-none text-xs text-secondary">
                      {$t('instance.manage.advancedSummary')}
                    </summary>
                    <div class="mt-2">
                      <div class="mb-1 flex items-center justify-between">
                        <label for="detail-min-heap" class="text-xs text-secondary">
                          {$t('instance.manage.minHeapLabel')}
                        </label>
                        <button
                          type="button"
                          class="btn-link text-xs"
                          onclick={() => {
                            minHeapDraft = heapDraft;
                            commitMinHeap();
                          }}
                        >
                          {$t('instance.manage.minHeapEqualsMax')}
                        </button>
                      </div>
                      <input
                        id="detail-min-heap"
                        type="number"
                        class="border rounded px-2 py-1 w-full text-sm"
                        min="0"
                        max={heapDraft}
                        placeholder={$t('instance.manage.minHeapPlaceholder')}
                        bind:value={minHeapDraft}
                        onchange={commitMinHeap}
                      />
                      <p class="mt-1 text-xs text-placeholder">
                        {$t('instance.manage.minHeapHint')}
                      </p>
                    </div>
                  </details>

                  <label
                    for="detail-jvm-args"
                    class="mb-1 flex items-center justify-between text-xs text-secondary"
                  >
                    <span>{$t('instance.manage.jvmArgsLabel')}</span>
                    {@render savedBadge('jvm')}
                  </label>
                  <input
                    id="detail-jvm-args"
                    class="border rounded px-2 py-1 w-full mb-3 font-mono text-xs"
                    placeholder={$t('instance.manage.jvmArgsPlaceholder')}
                    value={selected.extra_jvm_args}
                    oninput={() => clearSaved('jvm')}
                    onchange={(e) => setJvmArgs((e.currentTarget as HTMLInputElement).value)}
                  />

                  {#if selected.imported_from}
                    <div
                      class="mb-3 flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs"
                      data-testid="imported-provenance"
                    >
                      <Icon name="folderOpen" size={14} class="mt-0.5 shrink-0 text-muted" />
                      <div class="min-w-0 flex-1">
                        <div class="text-secondary">
                          {$t('instance.manage.importedFromLabel', {
                            launcher: displayLauncher(selected.imported_from.launcher),
                          })}
                        </div>
                        <div
                          class="truncate font-mono text-muted"
                          use:tooltip={{
                            text: selected.imported_from.source_path,
                            whenOverflowing: true,
                          }}
                        >
                          {selected.imported_from.source_path}
                        </div>
                      </div>
                      <button
                        type="button"
                        class="btn-secondary btn-xs inline-flex shrink-0 items-center gap-1"
                        onclick={openSourceFolder}
                        data-testid="open-source-folder-btn"
                      >
                        <Icon name="folderOpen" size={12} />
                        {$t('instance.manage.openSourceFolderBtn')}
                      </button>
                    </div>
                  {/if}

                  {#if selected.created_from_server}
                    <div
                      class="mb-3 flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs"
                      data-testid="created-from-server-provenance"
                    >
                      <Icon name="server" size={14} class="mt-0.5 shrink-0 text-muted" />
                      <div class="min-w-0 flex-1">
                        <div class="text-secondary">
                          {$t('instance.manage.fromServerLabel', {
                            name:
                              serverState.list.find((s) => s.id === selected.created_from_server)
                                ?.name ?? selected.created_from_server,
                          })}
                        </div>
                      </div>
                    </div>
                  {/if}

                  <div
                    data-focus-field="integrity"
                    use:fieldFlash={{
                      active: focusField === 'integrity',
                      focus: shouldFocusField('integrity'),
                    }}
                  >
                    <IntegritySection
                      instanceId={selected.id}
                      {isRunning}
                      name={selected.name}
                      status={selected.integrity}
                    />
                  </div>
                </div>
              </div>
            {:else}
              <p class="text-muted text-sm">{$t('instance.manage.emptyState')}</p>
            {/if}
          </div>
        </div>

        <div bind:this={errorRegionEl} class="shrink-0 px-4">
          <StatusMessage tone="danger" message={modalError} class="mb-3" />
        </div>
        {#if selected && !createMode}
          <div
            class="shrink-0 flex items-center justify-between border-t px-4 py-3"
            data-tour-ctx="manage-actions"
          >
            <span
              class="inline-flex"
              use:tooltip={{
                text: isRunning
                  ? $t('instance.manage.runningBlocked')
                  : instances.length <= 1
                    ? $t('instance.manage.cannotDeleteLast')
                    : '',
                describe: false,
              }}
            >
              <button
                type="button"
                class="btn-ghost-danger inline-flex items-center gap-1.5"
                disabled={instances.length <= 1 || isRunning}
                onclick={() => {
                  deleteConfirmTyped = '';
                  deleteConfirmOpen = true;
                }}
              >
                <Icon name="trash" size={14} />
                {$t('instance.manage.deleteBtn')}
              </button>
            </span>
            <div class="flex gap-2">
              <span
                class="inline-flex"
                use:tooltip={{
                  text: isRunning ? $t('instance.manage.runningBlocked') : '',
                  describe: false,
                }}
              >
                <button
                  type="button"
                  class="btn-secondary btn-sm inline-flex items-center gap-1.5"
                  disabled={isRunning}
                  onclick={() => selected && onCloneRequest(selected.id)}
                  data-testid="clone-instance-btn"
                >
                  <Icon name="copy" size={14} />
                  {$t('instance.manage.cloneBtn')}
                </button>
              </span>
              {#if onShortcutRequest}
                <button
                  type="button"
                  class="btn-secondary btn-sm inline-flex items-center gap-1.5"
                  onclick={() => selected && onShortcutRequest?.(selected.id)}
                  data-testid="create-shortcut-btn"
                >
                  <Icon name="monitor" size={14} />
                  {$t('shortcut.create')}
                </button>
              {/if}
              {#if onTranslationsRequest}
                <button
                  type="button"
                  class="btn-secondary btn-sm inline-flex items-center gap-1.5"
                  onclick={() => selected && onTranslationsRequest?.(selected.id)}
                  data-testid="manage-translations-btn"
                >
                  <Icon name="languages" size={14} />
                  {$t('instance.manage.translationsBtn')}
                </button>
              {/if}
              <button
                type="button"
                class="btn-secondary btn-sm inline-flex items-center gap-1.5"
                onclick={openFolder}
              >
                <Icon name="folderOpen" size={14} />
                {$t('instance.manage.openFolderBtn')}
              </button>
              <button type="button" class="btn-secondary btn-sm" onclick={close}>
                {$t('instance.manage.closeBtn')}
              </button>
            </div>
          </div>
        {/if}
      </section>
    </div>
  </Modal>
  <ContextualTour id="manage" steps={MANAGE_STEPS} />

  {#if deleteConfirmOpen && selected}
    <Modal
      ariaLabelledby="instance-delete-confirm-title"
      onClose={() => (deleteConfirmOpen = false)}
      panelClass="w-[440px] p-5 flex flex-col gap-3"
    >
      <h3 id="instance-delete-confirm-title" class="font-semibold text-primary text-base">
        {$t('instance.delete.title')}
      </h3>
      <p class="text-sm text-secondary">
        {$t('instance.delete.question', { name: selected.name })}
      </p>
      <p class="text-sm text-secondary">
        {$t('instance.delete.description')}
      </p>
      <label class="block text-xs text-secondary" for="instance-delete-confirm">
        {$t('worlds.delete.typeToConfirm', { word: DELETE_CONFIRM_WORD })}
      </label>
      <input
        id="instance-delete-confirm"
        class="border rounded px-2 py-1 w-full"
        bind:value={deleteConfirmTyped}
        placeholder={DELETE_CONFIRM_WORD}
        autocomplete="off"
        data-testid="instance-delete-confirm-input"
      />
      <div class="flex justify-end gap-2 mt-2">
        <button
          type="button"
          class="btn-secondary btn-sm"
          onclick={() => (deleteConfirmOpen = false)}
        >
          {$t('instance.manage.cancelBtn')}
        </button>
        <button
          type="button"
          class="btn-danger btn-sm"
          disabled={!canConfirmDelete}
          onclick={async () => {
            deleteConfirmOpen = false;
            await deleteSelected();
          }}
        >
          {$t('instance.delete.confirmBtn')}
        </button>
      </div>
    </Modal>
  {/if}

  {#if pendingChange !== null && selected}
    <Modal
      ariaLabelledby="instance-pack-detach-title"
      onClose={cancelPending}
      panelClass="w-[460px] p-5 flex flex-col gap-3"
    >
      <h3 id="instance-pack-detach-title" class="font-semibold text-primary text-base">
        {$t('instance.packDetach.title')}
      </h3>
      <p class="text-sm text-secondary">
        {pendingChange.kind === 'mc'
          ? $t('instance.packDetach.descriptionMc', { pack: selected.mrpack_name ?? '' })
          : $t('instance.packDetach.descriptionLoader', { pack: selected.mrpack_name ?? '' })}
      </p>
      <div class="flex justify-end gap-2 mt-2">
        <button type="button" class="btn-secondary btn-sm" onclick={cancelPending}>
          {$t('instance.manage.cancelBtn')}
        </button>
        <button type="button" class="btn-primary btn-sm" onclick={keepAndContinue}>
          {$t('instance.packDetach.keepBtn')}
        </button>
        <button type="button" class="btn-danger btn-sm" onclick={confirmDetachAndContinue}>
          {$t('instance.packDetach.detachBtn')}
        </button>
      </div>
    </Modal>
  {/if}
{/if}
