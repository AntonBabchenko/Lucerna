<script lang="ts">
  // Switch an installed pack to any published version, including a downgrade.
  //
  // This drives the SAME pipeline the "Update" fast path uses: the flow
  // controller (`createModpackUpdateFlow`) already accepts any
  // ModpackVersionEntry and the backend diffs whatever archive it is handed, in
  // either direction. Nothing here re-implements applying a pack version — the
  // dialog only chooses the target and states the risks.
  import type { InstanceWithStatus, ModpackVersionEntry } from '$lib/ipc/bindings';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import ChangelogModal from '$lib/mods/ChangelogModal.svelte';
  import { changelogSupported } from '$lib/mods/changelog-supported';
  import Modal from '$lib/ui/Modal.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { Icon } from '$lib/ui/icons';
  import ModpackDiffList from './ModpackDiffList.svelte';
  import ModpackUpdateProgress from './ModpackUpdateProgress.svelte';
  import ModpackVersionList from './ModpackVersionList.svelte';
  import SwitchRiskList from './SwitchRiskList.svelte';
  import { createModpackUpdateFlow } from './modpack-update-flow.svelte';
  import { assessSwitchRisks, switchChangelogRequest, switchDirection } from './switch-risks';

  let {
    inst,
    userAdded,
    manual,
    hasBundledFiles,
    onClose,
    onSwitched,
  }: {
    inst: InstanceWithStatus;
    /** Mods added via the Mod browser after import (drawer `provenance`). */
    userAdded: number;
    /** Jars dropped into `mods/` by hand (drawer `provenance`). */
    manual: number;
    /** The pack bundles `overrides/` content. */
    hasBundledFiles: boolean;
    onClose: () => void;
    onSwitched: () => void;
  } = $props();

  const flow = createModpackUpdateFlow();

  let versions = $state<ModpackVersionEntry[]>([]);
  let loadingVersions = $state(true);
  let loadError = $state<string | null>(null);
  let selected = $state<ModpackVersionEntry | null>(null);
  let changelogOpen = $state(false);

  const source = $derived(inst.mrpack_source ?? 'modrinth');

  const direction = $derived(
    selected === null ? 'unknown' : switchDirection(versions, inst.mrpack_version_id, selected.id),
  );

  const risks = $derived(
    flow.diff === null
      ? []
      : assessSwitchRisks({
          direction,
          versionBump: flow.diff.version_bump,
          userAdded,
          manual,
          hasBundledFiles,
        }),
  );

  const changelog = $derived(
    selected === null
      ? null
      : switchChangelogRequest(direction, inst.mrpack_version_id, selected.id),
  );

  const shownError = $derived(loadError ?? flow.error);

  // The review step is reachable only once the diff exists, so a slow network
  // cannot show an empty review panel.
  const step = $derived(flow.diff !== null ? 'review' : 'pick');

  async function loadVersions(projectId: string): Promise<void> {
    loadingVersions = true;
    loadError = null;
    const r = await commands.modpackGetVersions(source, projectId);
    if (r.status === 'ok') {
      versions = r.data;
    } else {
      loadError = formatError(r.error);
    }
    loadingVersions = false;
  }

  $effect(() => {
    const projectId = inst.mrpack_project_id;
    if (projectId === null) {
      loadingVersions = false;
      return;
    }
    void loadVersions(projectId);
  });

  async function pick(entry: ModpackVersionEntry): Promise<void> {
    selected = entry;
    await flow.prepare(inst, entry);
  }

  async function retry(): Promise<void> {
    if (selected === null) return;
    await flow.prepare(inst, selected);
  }

  function back(): void {
    flow.cancel();
    selected = null;
  }

  async function confirm(): Promise<void> {
    if (await flow.confirm(inst)) {
      onSwitched();
    }
  }
</script>

<Modal
  ariaLabelledby="modpack-switch-title"
  onClose={flow.busy ? () => {} : onClose}
  closeOnBackdrop={!flow.busy}
  closeOnEscape={!flow.busy}
  panelClass="w-[560px] max-h-[80vh] p-5 flex flex-col gap-3"
>
  <h3 id="modpack-switch-title" class="font-semibold text-base text-primary">
    {step === 'review' && selected !== null
      ? $t('modpacks.switch.reviewTitle', { version: selected.version_number })
      : $t('modpacks.switch.title', { pack: inst.mrpack_name ?? '' })}
  </h3>

  {#if shownError}
    <div class="flex items-center gap-2 text-sm text-danger" data-testid="switch-error">
      <span class="flex-1">{shownError}</span>
      {#if selected !== null}
        <button
          type="button"
          class="btn-secondary btn-xs flex-shrink-0"
          onclick={() => void retry()}
          data-testid="switch-retry"
        >
          {$t('modpacks.switch.retryBtn')}
        </button>
      {/if}
    </div>
  {/if}

  {#if flow.phase === 'applying'}
    <ModpackUpdateProgress progress={flow.progress} />
  {:else if step === 'pick'}
    {#if loadingVersions}
      <LoadingPanel label={$t('modpacks.switch.loadingVersions')} delayMs={0} />
    {:else}
      <ModpackVersionList
        {versions}
        installedVersionId={inst.mrpack_version_id}
        onSelect={(e) => void pick(e)}
      />
    {/if}
    <div class="flex justify-end">
      <button type="button" class="btn-secondary btn-sm" onclick={onClose}>
        {$t('modpacks.switch.cancelBtn')}
      </button>
    </div>
  {:else if flow.diff !== null && selected !== null}
    <SwitchRiskList {risks} />
    <div class="text-sm text-secondary">
      {$t('modpacks.update.changeSummary', {
        added: flow.diff.added.length,
        removed: flow.diff.removed.length,
        updated: flow.diff.updated.length,
      })}
    </div>
    <ModpackDiffList diff={flow.diff} />
    <div class="flex justify-end gap-2">
      <button type="button" class="btn-secondary btn-sm" onclick={back} data-testid="switch-back">
        {$t('modpacks.switch.backBtn')}
      </button>
      {#if inst.mrpack_project_id !== null && changelogSupported(source)}
        <button
          type="button"
          class="btn-ghost btn-sm inline-flex items-center gap-1"
          onclick={() => (changelogOpen = true)}
          data-testid="switch-changelog-btn"
        >
          <Icon name="scrollText" size={13} />
          {$t('mods.changelog.view')}
        </button>
      {/if}
      <button
        type="button"
        class="btn-warning btn-sm"
        onclick={() => void confirm()}
        data-testid="switch-confirm"
      >
        {$t('modpacks.switch.confirmBtn', { version: selected.version_number })}
      </button>
    </div>
  {/if}
</Modal>

<!-- Rendered AFTER the dialog it covers so Modal's escape stack closes this first. -->
{#if changelogOpen && changelog !== null && inst.mrpack_project_id !== null}
  <ChangelogModal
    {source}
    projectId={inst.mrpack_project_id}
    title={$t(changelog.titleKey)}
    targetVersionId={changelog.target}
    baseVersionId={changelog.base}
    onClose={() => (changelogOpen = false)}
  />
{/if}
