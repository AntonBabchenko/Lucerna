<script lang="ts">
  import { Channel } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { commands, events } from '$lib/ipc/bindings';
  import type {
    InstalledMod,
    InstanceWithStatus,
    ModpackProgress,
    ModpackStatus,
    ModpackUnresolvable,
    ModpackUpdateDiff,
    ModpackVersionEntry,
    ModSource,
    PackOriginFile,
    ProgressTick,
  } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import FindAlternativeDialog from '$lib/mods/FindAlternativeDialog.svelte';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import { Icon } from '$lib/ui/icons';
  import { drawerCache } from './drawer-cache';
  import { isUnresolvedMissingState } from './missing-mod';
  import ModpackUpdateDialog from './ModpackUpdateDialog.svelte';

  // Centered modal that surfaces the metadata captured at import time for a
  // pack-originated instance. Uses the shared Modal primitive (backdrop +
  // focus trap + Escape / backdrop close + role="dialog" on the panel); the
  // delete-confirm is a nested Modal on top.
  //
  // Source link rendering: a pack imported from the Modrinth Browse
  // flow lands with `mrpack_project_id` + `mrpack_source = 'modrinth'`
  // already populated, so the drawer can deep-link back to the
  // Modrinth (or CurseForge) project page without another API hop.
  // Drag-drop imports usually leave these null (the v0.5.0 sub-4 P1
  // orchestrator does a Modrinth-side auto-lookup by version_id, but
  // not every .mrpack hits that path) — in that case the link is
  // simply hidden.
  //
  // Mod list: hits `mods_list_installed`, which reconciles the on-disk
  // jars with the registry. Even mods that the user added by hand after
  // the import (drop into the instance's mods folder) show up here, so
  // the drawer stays accurate over time, not just immediately after
  // import.
  //
  // Provenance badges (bundle 2): `modpack_status` returns the frozen
  // pack-origin snapshot. We compute `originShas` once on each refresh
  // and use it to badge each installed row:
  //   - pack      — sha1 IS in originShas
  //   - + user    — sha1 NOT in originShas, mod has a source (added
  //                 via the Mod browser after import)
  //   - ? manual  — sha1 NOT in originShas, mod has no source (user
  //                 dropped a jar into mods/ by hand)
  //
  // Removed-from-pack section (bundle 2): if any sha in `origin.files`
  // is missing from `installed_shas`, render it under the installed
  // list with a Restore button that calls modpack_restore_file.
  //
  // Project-name enrichment (bundle 2): mirrors the workaround in
  // src/lib/mods/InstalledModsView.svelte — the registry stores a
  // version-shaped string in `m.name` for some mods (the Modrinth
  // version manifest doesn't always carry the display title). We
  // re-fetch via mods_project per installed-with-source mod, in
  // parallel, store the canonical title in a Map<sha1, name>, and use
  // it as the display name with fallback to `m.name`. Failures here
  // are silent — a transient lookup error keeps the registry name.
  //
  // Delete confirm overlay copy spells out the consequences (worlds,
  // configs, screenshots) — matches the ManageInstancesModal delete
  // dialog wording so the user gets the same warning regardless of
  // where they trigger the delete from. LastInstance errors from the
  // backend are surfaced via formatError; the button stays clickable
  // because we don't know the instance count from inside this drawer.

  let {
    inst,
    onClose,
    onOpenInstance,
    onDeleted,
    onUpdated,
  }: {
    inst: InstanceWithStatus;
    onClose: () => void;
    onOpenInstance: (id: string) => void;
    onDeleted: () => void;
    onUpdated?: () => void;
  } = $props();

  let mods = $state<InstalledMod[] | null>(null);
  let status = $state<ModpackStatus | null>(null);
  let nameMap = $state<Map<string, string>>(new Map());
  let restoreError = $state<string | null>(null);
  let findAltEntry = $state<ModpackUnresolvable | null>(null);
  let deleting = $state(false);
  // True only while the delete IPC is in flight — gates the confirm dialog's
  // backdrop/Escape close and disables its buttons so the destructive op
  // can't be re-fired or dismissed mid-flight.
  let deleteInFlight = $state(false);
  let deleteError = $state<string | null>(null);

  let updateAvailable = $state<ModpackVersionEntry | null>(null);
  let updateDiff = $state<ModpackUpdateDiff | null>(null);
  let updateTempPath = $state<string | null>(null);
  let updating = $state(false);
  let updateError = $state<string | null>(null);

  const originShas = $derived(
    new Set((status?.origin.files ?? []).map((f) => f.sha1.toLowerCase())),
  );

  // A pack file's category, from its install_path prefix. A resourcepack /
  // shaderpack is only a loadable `.zip` (or `.zip.disabled`); any other
  // file in those dirs (a `.zip.txt` download note, a stray `.rar`) is real
  // bundled content but not a pack of that type — it groups under "configs".
  type AssetCat = 'resourcepacks' | 'shaderpacks' | 'configs';
  function isZipPack(p: string): boolean {
    return p.endsWith('.zip') || p.endsWith('.zip.disabled');
  }
  function assetCat(installPath: string): AssetCat {
    if (installPath.startsWith('resourcepacks/') && isZipPack(installPath)) return 'resourcepacks';
    if (installPath.startsWith('shaderpacks/') && isZipPack(installPath)) return 'shaderpacks';
    return 'configs';
  }

  // install_paths currently flagged removed — excluded from the present
  // sections (they render in "Removed from pack" instead).
  const removedPaths = $derived(new Set((status?.removed_files ?? []).map((f) => f.install_path)));

  // Present (non-removed) pack files of one category. Mods are excluded —
  // they have their own disk-reality-driven section above.
  function presentAssets(cat: AssetCat): PackOriginFile[] {
    return (status?.origin.files ?? []).filter(
      (f) =>
        !f.install_path.startsWith('mods/') &&
        assetCat(f.install_path) === cat &&
        !removedPaths.has(f.install_path),
    );
  }

  $effect(() => {
    void inst.id;
    void load();
    void checkForUpdates();
  });

  // While the drawer is open, a mod installed or removed elsewhere
  // (e.g. a drag-drop local install on the Mods tab) can resolve or
  // un-resolve a "Mods to install manually" entry. Refresh silently so
  // the section and provenance badges stay live without a loading flash.
  onMount(() => {
    const unlisten = [
      events.modInstalled.listen(() => void load(true)),
      events.modUninstalled.listen(() => void load(true)),
    ];
    return () => {
      for (const u of unlisten) void u.then((fn) => fn());
    };
  });

  async function load(silent = false) {
    // Seed from the session cache so a reopened drawer renders instantly
    // instead of flashing "Loading…"; load() then revalidates below. A
    // silent refresh (fired by a mod-install event) likewise skips the
    // loading reset. Only a first-ever open with no cache entry shows
    // the "Loading…" state.
    const cached = drawerCache.get(inst.id);
    if (cached) {
      mods = cached.mods;
      status = cached.status;
      nameMap = cached.nameMap;
    } else if (!silent) {
      mods = null;
      status = null;
      nameMap = new Map();
    }
    restoreError = null;

    const [listR, statusR] = await Promise.all([
      commands.modsListInstalled(inst.id),
      commands.modpackStatus(inst.id),
    ]);

    if (listR.status === 'ok') {
      mods = listR.data;
    } else {
      // Fall through to "No mods installed" on error; the drawer's
      // primary purpose is pack metadata, not a mods debugger.
      mods = [];
    }
    if (statusR.status === 'ok') {
      status = statusR.data;
    } else {
      status = null;
    }

    // Project-name enrichment. Same shape as InstalledModsView: fetch
    // ModProject per installed mod with a source, drop errors silently,
    // build a map sha1 -> canonical name. Concurrent across all rows.
    // Always reassign `nameMap` (an empty map when there are no mods) so
    // a silent refresh down to zero mods can't leave stale entries.
    const next = new Map<string, string>();
    await Promise.all(
      mods.map(async (m) => {
        if (m.source == null || m.project_id == null) return;
        const p = await commands.modsProject(m.source as ModSource, m.project_id);
        if (p.status === 'ok') {
          next.set(m.sha1, p.data.summary.name);
        }
      }),
    );
    nameMap = next;
    // Store shallow copies of the collections so a later in-place
    // mutation of this drawer's own `mods` / `nameMap` can't reach back
    // and corrupt the cached snapshot. `status` is a read-only IPC value.
    drawerCache.set(inst.id, { mods: [...mods], status, nameMap: new Map(nameMap) });
  }

  async function checkForUpdates() {
    updateError = null;
    const r = await commands.modpackCheckUpdate(inst.id);
    if (r.status === 'ok') {
      updateAvailable = r.data;
    } else {
      updateError = formatError(r.error);
    }
  }

  // Fetch the new .mrpack + compute the diff, then open the dialog.
  async function openUpdateDialog() {
    if (!updateAvailable || !inst.mrpack_project_id) return;
    updateError = null;
    const fetched = await commands.modpackFetchToTemp(
      inst.mrpack_source ?? 'modrinth',
      inst.mrpack_project_id,
      updateAvailable.id,
    );
    if (fetched.status === 'error') {
      updateError = formatError(fetched.error);
      return;
    }
    updateTempPath = fetched.data;
    const d = await commands.modpackComputeUpdate(inst.id, updateTempPath);
    if (d.status === 'error') {
      updateError = formatError(d.error);
      return;
    }
    updateDiff = d.data;
  }

  async function applyUpdate() {
    if (!updateTempPath || !updateAvailable) return;
    const newVersionId = updateAvailable.id;
    updateDiff = null;
    updating = true;
    updateError = null;
    const phaseChannel = new Channel<ModpackProgress>();
    const tickChannel = new Channel<ProgressTick>();
    const r = await commands.modpackApplyUpdate(
      inst.id,
      updateTempPath,
      newVersionId,
      phaseChannel,
      tickChannel,
    );
    updating = false;
    if (r.status === 'error') {
      updateError = formatError(r.error);
      return;
    }
    updateAvailable = null;
    updateTempPath = null;
    await load();
    onUpdated?.();
  }

  let reimporting = $state(false);
  async function reimportPackFiles() {
    reimporting = true;
    restoreError = null;
    const phaseChannel = new Channel<ModpackProgress>();
    const r = await commands.modpackReimportOverrides(inst.id, phaseChannel);
    reimporting = false;
    if (r.status === 'error') {
      restoreError = formatError(r.error);
      return;
    }
    await load();
    onUpdated?.();
  }

  function sourceUrl(i: InstanceWithStatus): string | null {
    if (!i.mrpack_project_id || !i.mrpack_source) return null;
    if (i.mrpack_source === 'modrinth')
      return `https://modrinth.com/modpack/${i.mrpack_project_id}`;
    if (i.mrpack_source === 'curseforge')
      return `https://www.curseforge.com/projects/${i.mrpack_project_id}`;
    // FTB has no stable per-pack web URL keyed by numeric ID — hide the link.
    return null;
  }

  function sourceLabel(src: ModSource | null): string {
    if (src === 'modrinth') return 'Modrinth';
    if (src === 'curseforge') return 'CurseForge';
    if (src === 'ftb') return 'FTB';
    if (src === 'atlauncher') return 'ATLauncher';
    return '';
  }

  function formatBadge(src: ModSource | null): string {
    if (src === 'modrinth') return 'Modrinth .mrpack';
    if (src === 'curseforge') return 'CurseForge .zip';
    if (src === 'ftb') return 'FTB';
    if (src === 'atlauncher') return 'ATLauncher';
    return '.mrpack';
  }

  function displayName(m: InstalledMod): string {
    return nameMap.get(m.sha1) ?? m.name;
  }

  type Provenance = 'pack' | 'user' | 'manual';

  function provenance(m: InstalledMod): Provenance {
    if (originShas.has(m.sha1.toLowerCase())) return 'pack';
    if (m.source != null) return 'user';
    return 'manual';
  }

  async function restore(f: PackOriginFile) {
    restoreError = null;
    const r = await commands.modpackRestoreFile(inst.id, f.sha1);
    if (r.status === 'error') {
      restoreError = formatError(r.error);
      return;
    }
    // Re-load both lists so the file moves from removed → installed.
    await load();
  }

  async function confirmDelete() {
    if (deleteInFlight) return;
    deleteError = null;
    deleteInFlight = true;
    try {
      const r = await commands.deleteInstance(inst.id);
      if (r.status === 'ok') {
        deleting = false;
        onDeleted();
      } else {
        deleteError = formatError(r.error);
      }
    } finally {
      deleteInFlight = false;
    }
  }
</script>

<Modal
  ariaLabelledby="imported-detail-title"
  {onClose}
  dataTestid="imported-detail-drawer"
  panelClass="max-w-2xl w-full max-h-[85vh] overflow-y-auto flex flex-col"
>
  <header class="p-4 border-b flex items-start gap-3">
    <div class="flex-shrink-0"><Icon name="package" size={24} /></div>
    <div class="flex-1 min-w-0">
      <h3 id="imported-detail-title" class="font-semibold truncate">{inst.mrpack_name}</h3>
      <div class="text-xs text-muted truncate">
        v{inst.mrpack_version} · {formatBadge(inst.mrpack_source)}
      </div>
    </div>
    <CloseButton
      onClick={onClose}
      ariaLabel={$t('modpacks.imported.detail.closeAriaLabel')}
      class="flex-shrink-0"
    />
  </header>

  {#if inst.mrpack_summary}
    <div
      class="px-4 pt-3 pb-2 text-sm text-secondary selectable"
      data-testid="imported-detail-summary"
    >
      {inst.mrpack_summary}
    </div>
  {/if}

  {#if sourceUrl(inst)}
    {@const url = sourceUrl(inst)!}
    <div class="px-4 pb-3">
      <button
        type="button"
        onclick={() => void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url))}
        class="text-accent hover:underline text-sm inline-flex items-center gap-1"
        data-testid="imported-detail-source-link"
      >
        {$t('modpacks.imported.detail.openOn', { platform: sourceLabel(inst.mrpack_source) })}
        <Icon name="externalLink" size={14} />
      </button>
    </div>
  {/if}

  {#if updateError}
    <div class="px-4 pb-2 text-xs text-danger" data-testid="imported-detail-update-error">
      {updateError}
    </div>
  {/if}
  {#if updating}
    <div class="px-4 pb-3 text-sm text-accent" data-testid="imported-detail-updating">
      {$t('modpacks.imported.detail.updating')}
    </div>
  {:else if updateAvailable}
    <div class="px-4 pb-3">
      <div class="flex items-center gap-2 bg-accent-soft border border-accent rounded p-2 text-sm">
        <span class="flex-1 text-accent"
          >{$t('modpacks.imported.detail.updateAvailable', {
            version: updateAvailable.version_number,
          })}</span
        >
        <button
          type="button"
          class="btn-primary btn-xs"
          onclick={() => void openUpdateDialog()}
          data-testid="imported-detail-update-button"
        >
          {$t('modpacks.imported.detail.updateBtn')}
        </button>
      </div>
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto px-4 pb-4">
    {#snippet sectionSummary(label: string)}
      <summary
        class="font-medium text-sm text-primary cursor-pointer select-none list-none flex items-center gap-1 py-2"
      >
        <span class="disclosure-caret mr-1"><Icon name="caret" size={12} /></span>
        {label}
      </summary>
    {/snippet}

    {#if status && status.missing_mods.length > 0}
      <details class="mt-2" open data-testid="imported-detail-missing-section">
        {@render sectionSummary(
          $t('modpacks.imported.detail.missingHeading', {
            count: status.missing_mods.filter((m) => isUnresolvedMissingState(m.state)).length,
          }),
        )}
        <p class="text-xs text-muted mb-2 pl-4">
          {$t('modpacks.imported.detail.missingBody')}
        </p>
        <ul class="space-y-1 pl-4">
          {#each status.missing_mods as m (m.entry.mod_name + '|' + m.entry.filename)}
            {@const isInstalled = m.state === 'installed'}
            {@const isDifferentVersion = m.state === 'different_version'}
            {@const isSubstituted = m.state === 'substituted'}
            <li
              class="flex items-center gap-2 text-sm py-1 px-2 rounded border {isInstalled
                ? 'bg-success-bg border-success'
                : 'bg-warning-bg border-warning-text/30'}"
            >
              <!-- success when the mod is present at all (installed or a
                   different version); warning only when truly missing — so
                   "different version" is not mistaken for "missing". -->
              <Icon name={m.state === 'missing' ? 'warning' : 'success'} class="flex-shrink-0" />
              <span class="truncate flex-1" class:text-muted={isInstalled}>
                {m.entry.mod_name}
                {#if isDifferentVersion}
                  <span class="text-muted text-xs">
                    {$t('modpacks.imported.detail.differentVersion')}</span
                  >
                {/if}
                {#if isSubstituted}
                  <span class="text-muted text-xs"
                    >{$t('modpacks.imported.detail.substituted')}</span
                  >
                {/if}
              </span>
              {#if !isInstalled}
                <span
                  class="text-[10px] px-1.5 py-0.5 rounded bg-subtle text-secondary flex-shrink-0"
                >
                  {m.entry.reason === 'distribution_disabled'
                    ? $t('modpacks.imported.detail.distributionDisabled')
                    : $t('modpacks.imported.detail.hostNotAllowed')}
                </span>
              {/if}
              {#if !isInstalled && m.entry.manual_action_url}
                <button
                  type="button"
                  onclick={() =>
                    void import('@tauri-apps/plugin-opener').then((opener) =>
                      opener.openUrl(m.entry.manual_action_url!),
                    )}
                  class="text-accent hover:underline text-xs flex-shrink-0 inline-flex items-center gap-1"
                >
                  {$t('modpacks.imported.detail.openLink')}
                  <Icon name="externalLink" size={12} />
                </button>
              {/if}
              {#if isUnresolvedMissingState(m.state)}
                <button
                  type="button"
                  class="text-accent hover:underline text-xs flex-shrink-0"
                  data-testid="find-alt-open"
                  onclick={() => (findAltEntry = m.entry)}
                >
                  {$t('modpacks.imported.detail.findAlternative')}
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      </details>
    {/if}

    {#if status && status.removed_files.length > 0}
      <details class="mt-2" open data-testid="imported-detail-removed-section">
        {@render sectionSummary(
          $t('modpacks.imported.detail.removedHeading', { count: status.removed_files.length }),
        )}
        {#if restoreError}
          <p class="text-xs text-danger mb-2 pl-4" data-testid="imported-detail-restore-error">
            {restoreError}
          </p>
        {/if}
        <ul class="space-y-1 pl-4">
          {#each status.removed_files as f (f.sha1)}
            <li
              class="flex items-center gap-2 text-sm py-1 px-2 rounded bg-danger-bg border border-danger"
            >
              <span class="truncate flex-1 line-through text-secondary">{f.name}</span>
              {#if f.url === ''}
                <!-- Bundled mod from overrides/mods/ — no network source
                     to re-fetch from. Disable Restore and explain why on
                     hover; the user's only recovery path is re-importing
                     the .mrpack. -->
                <button
                  type="button"
                  class="btn-secondary btn-xs flex-shrink-0"
                  disabled
                  title={$t('modpacks.imported.detail.restoreDisabledTitle')}
                  data-testid="imported-detail-restore-{f.sha1}"
                >
                  {$t('modpacks.imported.detail.restoreBtn')}
                </button>
              {:else}
                <button
                  type="button"
                  class="btn-secondary btn-xs flex-shrink-0"
                  onclick={() => void restore(f)}
                  data-testid="imported-detail-restore-{f.sha1}"
                >
                  {$t('modpacks.imported.detail.restoreBtn')}
                </button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if status.removed_files.some((f) => f.url === '')}
          <button
            type="button"
            class="btn-secondary btn-xs mt-2 ml-4"
            onclick={() => void reimportPackFiles()}
            disabled={reimporting}
            data-testid="imported-detail-reimport"
          >
            {reimporting
              ? $t('modpacks.imported.detail.reimporting')
              : $t('modpacks.imported.detail.reimportBtn')}
          </button>
        {/if}
      </details>
    {/if}

    {#if status && (status.origin.skipped_overrides?.length ?? 0) > 0}
      {@const skipped = status.origin.skipped_overrides ?? []}
      <details class="mt-2" open data-testid="imported-detail-skipped-section">
        {@render sectionSummary(
          $t('modpacks.imported.detail.skippedHeading', {
            count: skipped.length,
          }),
        )}
        <p class="text-xs text-muted mb-2 pl-4">
          {$t('modpacks.imported.detail.skippedBody')}
        </p>
        <ul class="space-y-1 pl-4">
          {#each skipped as s (s.path)}
            <li
              class="flex items-center gap-2 text-sm py-1 px-2 rounded border bg-subtle border-border-subtle text-secondary"
            >
              <Icon name="info" class="flex-shrink-0" />
              <span class="truncate flex-1">{s.path}</span>
              <span class="text-xs text-muted flex-shrink-0">{formatSize(s.size)}</span>
            </li>
          {/each}
        </ul>
      </details>
    {/if}

    <details class="mt-2">
      {@render sectionSummary(
        $t('modpacks.imported.detail.modsHeading', { count: mods?.length ?? 0 }),
      )}
      {#if mods === null}
        <div class="text-sm text-muted pl-4" data-testid="imported-detail-mods-loading">
          {$t('modpacks.imported.detail.loading')}
        </div>
      {:else if mods.length === 0}
        <div class="text-sm text-muted pl-4" data-testid="imported-detail-mods-empty">
          {$t('modpacks.imported.detail.modsEmpty')}
        </div>
      {:else}
        <ul class="space-y-1 pl-4" data-testid="imported-detail-mods-list">
          {#each mods as m (m.sha1)}
            {@const prov = provenance(m)}
            <li class="flex items-center gap-2 text-sm py-1">
              <div
                class="w-2 h-2 rounded-full flex-shrink-0"
                class:bg-accent={m.enabled}
                class:bg-muted={!m.enabled}
                aria-hidden="true"
              ></div>
              {#if prov === 'pack'}
                <span
                  class="inline-flex items-center gap-1 text-[10px] font-medium px-1.5 py-0.5 rounded bg-accent-soft text-accent flex-shrink-0"
                  title={$t('modpacks.imported.detail.badgeFromPackTitle')}
                  data-testid="mod-badge-pack-{m.sha1}"
                >
                  <Icon name="package" size={11} />
                  {$t('modpacks.imported.detail.badgeFromPack')}
                </span>
              {:else if prov === 'user'}
                <span
                  class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-success-bg text-success flex-shrink-0"
                  title={$t('modpacks.imported.detail.badgeUserAddedTitle')}
                  data-testid="mod-badge-user-{m.sha1}"
                >
                  {$t('modpacks.imported.detail.badgeUserAdded')}
                </span>
              {:else}
                <span
                  class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-subtle text-secondary flex-shrink-0"
                  title={$t('modpacks.imported.detail.badgeManualTitle')}
                  data-testid="mod-badge-manual-{m.sha1}"
                >
                  {$t('modpacks.imported.detail.badgeManual')}
                </span>
              {/if}
              <span class="truncate flex-1">{displayName(m)}</span>
              {#if !m.enabled}
                <span
                  class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-subtle text-muted flex-shrink-0"
                >
                  {$t('modpacks.imported.detail.disabled')}
                </span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </details>

    {#if status}
      {@const resourcepacks = presentAssets('resourcepacks')}
      {@const shaderpacks = presentAssets('shaderpacks')}
      {@const configs = presentAssets('configs')}

      {#if resourcepacks.length > 0}
        <details class="mt-2" data-testid="imported-detail-resourcepacks-section">
          {@render sectionSummary(
            $t('modpacks.imported.detail.resourcePacksHeading', { count: resourcepacks.length }),
          )}
          <ul class="space-y-1 pl-4" data-testid="imported-detail-resourcepacks">
            {#each resourcepacks as f (f.install_path)}
              <li class="flex items-center gap-2 text-sm py-1">
                <span class="truncate flex-1">{f.name}</span>
                <span class="text-xs text-placeholder flex-shrink-0">{formatSize(f.size)}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}

      {#if shaderpacks.length > 0}
        <details class="mt-2" data-testid="imported-detail-shaderpacks-section">
          {@render sectionSummary(
            $t('modpacks.imported.detail.shaderPacksHeading', { count: shaderpacks.length }),
          )}
          <ul class="space-y-1 pl-4" data-testid="imported-detail-shaderpacks">
            {#each shaderpacks as f (f.install_path)}
              <li class="flex items-center gap-2 text-sm py-1">
                <span class="truncate flex-1">{f.name}</span>
                <span class="text-xs text-placeholder flex-shrink-0">{formatSize(f.size)}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}

      {#if configs.length > 0}
        <details class="mt-2" data-testid="imported-detail-configs-section">
          {@render sectionSummary(
            $t('modpacks.imported.detail.configsHeading', { count: configs.length }),
          )}
          <ul class="space-y-1 pl-4" data-testid="imported-detail-configs">
            {#each configs as f (f.install_path)}
              <li class="flex items-center gap-2 text-sm py-1">
                <span class="truncate flex-1 text-secondary">{f.install_path}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}
    {/if}
  </div>

  <footer
    class="p-4 border-t flex justify-between items-center sticky bottom-0 bg-surface flex-shrink-0"
  >
    <button
      type="button"
      class="btn-ghost-danger"
      onclick={() => (deleting = true)}
      data-testid="imported-detail-delete"
    >
      {$t('modpacks.imported.detail.deleteBtn')}
    </button>
    <button
      type="button"
      class="btn-primary btn-sm"
      onclick={() => onOpenInstance(inst.id)}
      data-testid="imported-detail-open"
    >
      {$t('modpacks.imported.detail.openInstanceBtn')}
    </button>
  </footer>
</Modal>

{#if updateDiff}
  <ModpackUpdateDialog
    diff={updateDiff}
    onCancel={() => {
      updateDiff = null;
      updateTempPath = null;
    }}
    onConfirm={() => void applyUpdate()}
  />
{/if}

{#if findAltEntry}
  <FindAlternativeDialog
    modName={findAltEntry.mod_name}
    mcVersion={inst.mc_version}
    loader={inst.loader}
    instanceId={inst.id}
    curseForgeUrl={findAltEntry.manual_action_url || null}
    onClose={() => (findAltEntry = null)}
    onInstalled={async ({ source, projectId }) => {
      const entry = findAltEntry;
      if (!entry) return;
      const r = await commands.modpackResolveMissingWith(
        inst.id,
        entry.filename,
        entry.mod_name,
        source,
        projectId,
      );
      if (r.status === 'error') {
        pushWarning($t('modpacks.imported.detail.findAlternative'), [formatError(r.error)]);
      }
      await load(true);
    }}
  />
{/if}

{#if deleting}
  <Modal
    ariaLabelledby="imported-delete-confirm-title"
    onClose={() => {
      deleting = false;
      deleteError = null;
    }}
    closeOnBackdrop={!deleteInFlight}
    closeOnEscape={!deleteInFlight}
    panelClass="w-[440px] p-5 flex flex-col gap-3"
  >
    <h3 id="imported-delete-confirm-title" class="font-semibold text-base">
      {$t('modpacks.imported.detail.deleteConfirmTitle')}
    </h3>
    <p class="text-sm text-secondary">
      {$t('modpacks.imported.detail.deleteConfirmBody')}
    </p>
    {#if deleteError}
      <p class="text-xs text-danger" data-testid="imported-detail-delete-error">
        {deleteError}
      </p>
    {/if}
    <div class="flex justify-end gap-2 mt-2">
      <button
        type="button"
        class="btn-secondary btn-sm"
        disabled={deleteInFlight}
        onclick={() => {
          deleting = false;
          deleteError = null;
        }}
        data-testid="imported-detail-delete-cancel"
      >
        {$t('common.cancel')}
      </button>
      <button
        type="button"
        class="btn-danger btn-sm"
        disabled={deleteInFlight}
        onclick={confirmDelete}
        data-testid="imported-detail-delete-confirm"
      >
        {$t('modpacks.imported.detail.deleteConfirmBtn')}
      </button>
    </div>
  </Modal>
{/if}
