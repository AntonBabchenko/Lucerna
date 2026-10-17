<script lang="ts">
  // The Game section of the Settings tab: Minecraft version, loader, the mod
  // compatibility summary and its fix, and the pack-detach question. Moved from
  // the Manage window, whose comments on each mechanism are kept below.
  import { onDestroy, untrack } from 'svelte';
  import { get } from 'svelte/store';
  import {
    commands,
    type InstanceWithStatus,
    type LoaderKind,
    type VersionEntry,
  } from '$lib/ipc/bindings';
  import { compatSummaryFromScan, loaderOutcomeToast } from '$lib/instances/integrity-messages';
  import { instanceIpcErrorMessage } from '$lib/instances/instance-ipc-error';
  import { displayLoader } from '$lib/instances/loader-display';
  import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
  import { shouldFocusField, type ManageFocusField } from '$lib/instances/manage-focus';
  import { compatScanEntries, ensureCompatScan } from '$lib/mods/compat-scan.svelte';
  import MigrationPlanDialog from '$lib/mods/MigrationPlanDialog.svelte';
  import { t } from '$lib/i18n';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { fieldFlash } from '$lib/ui/field-flash';
  import Modal from '$lib/ui/Modal.svelte';
  import Select from '$lib/ui/Select.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import { cfgBusy, cfgSuperseded, enqueueCfgChange } from './cfg-queue.svelte';
  import { reportLateFailure } from './late-failure';
  import VersionListNotice from '$lib/versions/VersionListNotice.svelte';
  import { LOADED_VERSION_LIST, type VersionListState } from '$lib/versions/mc-versions.svelte';

  let {
    instance,
    versions,
    versionList = LOADED_VERSION_LIST,
    running,
    busy,
    focusField = null,
    onFocusDelivered = () => {},
    onChanged,
  }: {
    instance: InstanceWithStatus;
    versions: VersionEntry[];
    /** Loading / failed state of `versions`, said under the picker when it shows none. */
    versionList?: VersionListState;
    /** This instance's game is up: its version and loader stay as they are. */
    running: boolean;
    /** A task runs on this instance (install, verify, mod install…). */
    busy: boolean;
    focusField?: ManageFocusField | null;
    onFocusDelivered?: () => void;
    onChanged: () => void;
  } = $props();

  const locked = $derived(running || busy);

  // A version or loader change can still be in flight while the fields show
  // the SAVED config, not the one about to land: the section says a change is
  // being applied. That is every version change (the version field shows the
  // saved value until the change lands) and a loader change started before
  // this mount (an earlier visit to the tab). Not a lock: a click queues behind
  // it and the last click wins. A loader change the user clicked in this mount
  // needs no line — the picker already shows the click.
  let startedHere = $state(false);
  const joinedInFlight = $derived(cfgBusy(instance.id) && !startedHere);
  $effect(() => {
    if (!cfgBusy(instance.id)) startedHere = false;
  });

  // The message for an IPC call that threw instead of answering.
  const thrownMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

  // A completion that lands after the tab unmounted (another tab was clicked)
  // or after the active instance changed must not write this section's UI.
  // Plain `let`: a `$state` read in the destroy phase is stale.
  let unmounted = false;
  onDestroy(() => {
    unmounted = true;
  });
  function isStale(id: string): boolean {
    return unmounted || instance.id !== id;
  }

  // The section's one message slot, about the LATEST attempt: every change
  // clears it as it starts and its failure, if any, writes it.
  let error = $state<string | null>(null);
  let errorEl = $state<HTMLElement | null>(null);
  // Scroll the region into view when an error first appears — a picker failure
  // can land seconds after the user scrolled away. Gate on the null → message
  // edge (plain cursor) so a second failure does not yank the viewport.
  let lastScrolledError: string | null = null;
  $effect(() => {
    if (error && !lastScrolledError && errorEl) errorEl.scrollIntoView?.({ block: 'nearest' });
    lastScrolledError = error;
  });

  // Snapshot toggle for the MC version picker. Off by default — most users
  // want stable releases.
  let showSnapshots = $state(false);
  const visibleVersions = $derived(
    versions.filter((v) => (showSnapshots ? true : v.version_type === 'release')),
  );
  // Keep the empty "Choose…" row so Select greys the trigger of an instance
  // that has no version yet. Picking it writes nothing (setMc).
  const mcVersionOptions = $derived([
    { value: '', label: $t('instance.manage.chooseMcOption') },
    ...visibleVersions.map((v) => ({ value: v.id, label: v.id })),
  ]);

  // Mod-compat summary after an MC/loader change. Sourced from the shared
  // offline scan (`compat-scan.svelte`), not a platform query: the platform
  // answers "does this PROJECT have a build for (mc, loader)?", which every mod
  // in the original bug report could answer yes to even though the FILE on
  // disk was still the old build. Read through, not snapshotted (spec D6): a
  // late-landing scan fills the summary in reactively.
  let compatChecked = $state(false);
  // Set when the scan could not be computed. `ensureCompatScan` absorbs an
  // ordinary backend error itself, so this is only reached by a transport-level
  // throw — rare, but real enough for the quiet "couldn't check" note.
  let compatCheckFailed = $state(false);
  const compatWarningText = $derived(
    compatChecked
      ? compatSummaryFromScan(compatScanEntries(), compatScanEntries().length, instance.loader)
      : null,
  );
  // MigrationPlanDialog acts on the instance it was opened for, captured then:
  // an outside switch of the active instance must not redirect it.
  let migrationFor = $state<string | null>(null);

  // A different instance: nothing shown here is about it any more.
  let lastId: string | null = null;
  $effect(() => {
    const id = instance.id;
    untrack(() => {
      if (id === lastId) return;
      lastId = id;
      compatChecked = false;
      compatCheckFailed = false;
      error = null;
      startedHere = false;
      takePending(false);
    });
  });

  // Takes the NEW mc/loader explicitly rather than reading `instance`: after a
  // change, `onChanged()` refreshes the list through an async round-trip that
  // is not awaited here, so `instance` still holds the OLD config.
  async function runModCompatCheck(id: string, mc: string, loader: LoaderKind) {
    try {
      // force: true — the mod set did not change the scan key, so an unforced
      // call would be deduplicated away against a scan from before the change.
      await ensureCompatScan(id, mc, loader, { force: true });
      if (isStale(id)) return;
      compatChecked = true;
      compatCheckFailed = false;
    } catch {
      // ensureCompatScan absorbs an ordinary backend error and keeps the
      // previous scan in place; reaching here means the IPC call itself threw.
      if (isStale(id)) return;
      compatChecked = false;
      compatCheckFailed = true;
    }
  }

  // Pending pack-detach question. `instanceId` is fixed when the question is
  // ASKED: the answer must land on the instance it was asked about.
  type PendingChange =
    | { kind: 'mc'; instanceId: string; name: string; value: string }
    | {
        kind: 'loader';
        instanceId: string;
        name: string;
        loaderKind: LoaderKind;
        loaderVersion: string | null;
        // Same loader, another build: the prompt must not claim the loader changes.
        versionOnly: boolean;
        // Answers the LoaderPicker's commit request once the user has decided.
        settle: (accepted: boolean) => void;
      };
  let pendingChange = $state<PendingChange | null>(null);

  // Every way out of the prompt goes through here, so a picker waiting for its
  // verdict is never left hanging. `proceed === false` answers it right away;
  // on `true` the caller answers once the write it starts has an outcome.
  function takePending(proceed: boolean): PendingChange | null {
    const change = pendingChange;
    pendingChange = null;
    if (change?.kind === 'loader' && !proceed) change.settle(false);
    return change;
  }

  // An MC change. Last click wins (cfg-queue): an overtaken change stays
  // silent. One that is not overtaken refreshes the list and shows its outcome
  // even after the tab unmounted — toasts are page-level; only this section's
  // own UI (error slot, compat summary) needs it still here, on this instance.
  async function applyMcChange(id: string, name: string, mc: string) {
    await enqueueCfgChange(id, async () => {
      if (!isStale(id)) error = null;
      const fail = (message: string) => {
        if (isStale(id)) reportLateFailure(name, get(t)('instance.manage.mcVersionLabel'), message);
        else error = message;
      };
      let result;
      try {
        result = await commands.changeInstanceMc(id, mc);
      } catch (e) {
        if (!cfgSuperseded(id)) fail(thrownMessage(e));
        return;
      }
      if (cfgSuperseded(id)) return;
      if (result.status === 'ok') {
        const toast = loaderOutcomeToast(result.data.loader_outcome, mc);
        if (toast?.kind === 'success') pushSuccess(toast.text);
        else if (toast?.kind === 'warning') pushWarning(toast.text);
        onChanged();
        // Only while still shown: the page rescans the ACTIVE instance on an
        // MC/loader change itself, and a scan for an instance no longer shown
        // would land in the one shared store under another instance's Overview.
        const fresh = result.data.instance;
        if (!isStale(id)) await runModCompatCheck(fresh.id, fresh.mc_version, fresh.loader);
      } else {
        fail(instanceIpcErrorMessage(result.error));
      }
    });
  }

  async function setMc(mc: string) {
    // The empty "Choose…" row: a version is never cleared from here.
    if (!mc || mc === instance.mc_version) return;
    const { id, name } = instance;
    if (instance.mrpack_name) {
      takePending(false);
      pendingChange = { kind: 'mc', instanceId: id, name, value: mc };
      return;
    }
    await applyMcChange(id, name, mc);
  }

  // Resolves to whether the loader was WRITTEN — the LoaderPicker's verdict.
  // The promise is tied to the queued entry itself, not to the queue's return:
  // a discarded entry answers false, and an overtaken one still reports its
  // real outcome while staying silent otherwise.
  function applyLoaderChange(
    id: string,
    name: string,
    kind: LoaderKind,
    version: string | null,
  ): Promise<boolean> {
    return new Promise<boolean>((resolve) => {
      void enqueueCfgChange(
        id,
        async () => {
          const fail = (message: string) => {
            if (isStale(id)) reportLateFailure(name, get(t)('instance.loader.label'), message);
            else error = message;
          };
          try {
            if (!isStale(id)) error = null;
            let result;
            try {
              result = await commands.setInstanceLoader(id, kind, version);
            } catch (e) {
              if (!cfgSuperseded(id)) fail(thrownMessage(e));
              return;
            }
            resolve(result.status === 'ok');
            if (cfgSuperseded(id)) return;
            if (result.status === 'ok') {
              onChanged();
              if (!isStale(id))
                await runModCompatCheck(result.data.id, result.data.mc_version, result.data.loader);
            } else {
              fail(instanceIpcErrorMessage(result.error));
            }
          } finally {
            // Reached unanswered only if the IPC call itself threw: nothing was
            // written as far as anyone can tell. (A no-op after the resolve.)
            resolve(false);
          }
        },
        () => resolve(false),
      );
    });
  }

  // The LoaderPicker's commit request. The verdict is what lets the picker go
  // back to the saved loader when the change is refused, cancelled or fails.
  function commitLoader(
    kind: LoaderKind,
    version: string | null,
    origin: 'user' | 'auto',
  ): Promise<boolean> {
    // Validate up front so a missing MC version fails fast instead of after the
    // detach prompt.
    if (kind !== 'vanilla' && !instance.mc_version) {
      error = get(t)('instance.error.pickMcFirst');
      return Promise.resolve(false);
    }
    const { id, name } = instance;
    if (!instance.mrpack_name) {
      // Not the picker correcting a saved version by itself: that shows no click.
      if (origin === 'user') startedHere = true;
      return applyLoaderChange(id, name, kind, version);
    }
    // A modpack instance changes only on the user's say-so. `auto` is the
    // picker correcting a saved version the loader no longer offers: rewriting a
    // pack-pinned build silently is wrong, and so is raising an irreversible
    // keep/detach question nobody asked. Refused, the picker shows the saved
    // version as not on offer, and the user can pick one — which does ask.
    if (origin === 'auto') return Promise.resolve(false);
    const versionOnly = kind === instance.loader;
    return new Promise<boolean>((settle) => {
      takePending(false);
      pendingChange = {
        kind: 'loader',
        instanceId: id,
        name,
        loaderKind: kind,
        loaderVersion: version,
        versionOnly,
        settle,
      };
    });
  }

  async function applyPending(change: PendingChange) {
    if (change.kind === 'mc') {
      await applyMcChange(change.instanceId, change.name, change.value);
      return;
    }
    if (!isStale(change.instanceId)) startedHere = true;
    change.settle(
      await applyLoaderChange(
        change.instanceId,
        change.name,
        change.loaderKind,
        change.loaderVersion,
      ),
    );
  }

  async function confirmDetachAndContinue() {
    const change = takePending(true);
    if (!change) return;
    const id = change.instanceId;
    error = null;
    try {
      const detachResult = await commands.detachInstancePack(id);
      if (detachResult.status === 'error') {
        // Not detached: the change it gated does not run. The `finally` tells
        // the picker no.
        const message = instanceIpcErrorMessage(detachResult.error);
        if (isStale(id))
          reportLateFailure(change.name, $t('instance.packDetach.detachLabel'), message);
        else error = message;
        return;
      }
      // The change is applied by id, so it does not wait for onChanged()'s
      // refresh of `instance`.
      onChanged();
      await applyPending(change);
    } finally {
      // Every exit that did not reach applyPending's answer — an early return
      // above, or the IPC call itself throwing — is a "no". A no-op once the
      // real answer is in.
      if (change.kind === 'loader') change.settle(false);
    }
  }

  async function keepAndContinue() {
    const change = takePending(true);
    if (change) await applyPending(change);
  }

  function cancelPending() {
    takePending(false);
  }
</script>

<section class="flex flex-col">
  <h3 class="mb-2 text-sm font-medium text-primary" data-tour-ctx="settings-game">
    {$t('instance.settings.gameSection')}
  </h3>
  {#if running}
    <p class="mb-2 text-xs text-secondary" data-testid="settings-game-locked">
      {$t('instance.settings.lockedRunning')}
    </p>
  {:else if busy}
    <p class="mb-2 text-xs text-secondary" data-testid="settings-game-locked">
      {$t('instance.settings.lockedBusy')}
    </p>
  {:else if joinedInFlight}
    <p class="mb-2 text-xs text-secondary" data-testid="settings-game-applying">
      {$t('instance.settings.applyingChange')}
    </p>
  {/if}

  <!-- The flash zone spans label + control + the snapshots toggle because they
       read as one field; the no-versions notice stays outside so an empty-list
       warning is never highlighted as "the thing you asked for". -->
  <div
    data-focus-field="mc"
    use:fieldFlash={{
      active: focusField === 'mc',
      focus: shouldFocusField('mc'),
      onDelivered: onFocusDelivered,
    }}
  >
    <label for="detail-mc-version" class="block text-xs text-secondary mb-1"
      >{$t('instance.manage.mcVersionLabel')}</label
    >
    <span
      class="block mb-1"
      use:tooltip={{ text: running ? $t('instance.manage.runningBlocked') : '', describe: false }}
    >
      <Select
        id="detail-mc-version"
        class="w-full"
        value={instance.mc_version}
        options={mcVersionOptions}
        disabled={locked}
        onChange={(v) => setMc(String(v))}
      />
    </span>
    <label class="text-xs flex items-center gap-1 mb-3">
      <input type="checkbox" bind:checked={showSnapshots} />
      {$t('instance.manage.showSnapshots')}
    </label>
  </div>
  {#if visibleVersions.length === 0}
    <VersionListNotice
      total={versions.length}
      loading={versionList.loading}
      failure={versionList.failure}
      onReload={versionList.reload}
      pickerId="detail-mc-version"
    />
  {/if}

  <!--
    Keyed on the instance id so the picker REMOUNTS when the active instance
    changes: its uncommitted view, its pending verdicts and its fetched list all
    belong to ONE instance, and none of them may leak into the next.
    (Historically a leaked value made swapping to a modpack instance read as a
    loader change and falsely raised the pack-detach prompt.)
  -->
  <div
    data-focus-field="loader"
    use:fieldFlash={{
      active: focusField === 'loader',
      focus: shouldFocusField('loader'),
      onDelivered: onFocusDelivered,
    }}
  >
    <span
      class="block"
      use:tooltip={{ text: running ? $t('instance.manage.runningBlocked') : '', describe: false }}
    >
      {#key instance.id}
        <LoaderPicker
          mc={instance.mc_version}
          loader={instance.loader}
          loaderVersion={instance.loader_version}
          disabled={locked}
          onchange={commitLoader}
        />
      {/key}
    </span>
  </div>

  <!-- Compat summary + check-failure note share a polite live region so screen
       readers hear the advisory; both are empty in the idle state. -->
  <StatusMessage
    tone="warning"
    live="polite"
    withIcon
    message={compatWarningText}
    class="bg-warning-bg border border-warning-text/30 rounded px-2 py-1.5 mt-2 mb-1"
  />
  {#if compatWarningText}
    <div class="flex justify-end mb-1">
      <button
        type="button"
        class="btn-tertiary btn-xs"
        data-testid="open-migration-plan-btn"
        onclick={() => (migrationFor = instance.id)}
      >
        {$t('mods.migration.openBtn')}
      </button>
    </div>
  {/if}
  <StatusMessage
    tone="info"
    live="polite"
    message={compatCheckFailed ? $t('instance.manage.compatCheckUnavailable') : null}
    class="mt-2 mb-1"
  />

  <StatusMessage bind:element={errorEl} tone="danger" message={error} class="mb-2" />
</section>

{#if pendingChange !== null}
  <Modal
    ariaLabelledby="instance-pack-detach-title"
    ariaDescribedby="instance-pack-detach-desc"
    onClose={cancelPending}
    panelClass="w-[480px] max-w-full p-5 flex flex-col gap-3"
  >
    <h3 id="instance-pack-detach-title" class="font-semibold text-primary text-base">
      {pendingChange.kind === 'mc'
        ? $t('instance.packDetach.titleMc', { to: pendingChange.value })
        : pendingChange.versionOnly
          ? $t('instance.packDetach.titleLoaderVersion', { to: pendingChange.loaderVersion ?? '' })
          : $t('instance.packDetach.titleLoader', {
              to: displayLoader(pendingChange.loaderKind),
            })}
    </h3>
    <!-- Each choice gets its own paragraph opening with the very words on its
         button, so "what does this button do" is answered next to it. -->
    <div id="instance-pack-detach-desc" class="flex flex-col gap-2 text-sm text-secondary">
      <p>{$t('instance.packDetach.intro', { pack: instance.mrpack_name ?? '' })}</p>
      <p>
        <strong class="text-primary">{$t('instance.packDetach.keepLabel')}</strong> —
        {$t('instance.packDetach.keepBody')}
      </p>
      <p>
        <strong class="text-primary">{$t('instance.packDetach.detachLabel')}</strong> —
        {$t('instance.packDetach.detachBody')}
      </p>
    </div>
    <div class="flex justify-end gap-2 mt-2">
      <button type="button" class="btn-secondary btn-sm" onclick={cancelPending}>
        {$t('instance.packDetach.cancelBtn')}
      </button>
      <button type="button" class="btn-primary btn-sm" onclick={keepAndContinue}>
        {$t('instance.packDetach.keepLabel')}
      </button>
      <button type="button" class="btn-danger btn-sm" onclick={confirmDetachAndContinue}>
        {$t('instance.packDetach.detachLabel')}
      </button>
    </div>
  </Modal>
{/if}

{#if migrationFor !== null}
  {@const target = migrationFor}
  <MigrationPlanDialog
    instanceId={target}
    onClose={() => (migrationFor = null)}
    onApplied={() => {
      // Refresh the summary so it reflects what the migration just fixed — only
      // while that instance is still the one shown (see applyMcChange).
      if (!isStale(target)) void runModCompatCheck(target, instance.mc_version, instance.loader);
    }}
  />
{/if}
