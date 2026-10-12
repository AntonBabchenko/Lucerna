<script lang="ts">
  // The client's fifth tab: the active instance's settings. It replaced the
  // Manage window (site feedback #4, 2026-10-11 — "move manage next to
  // screenshots and name it settings"). The tab always shows the ACTIVE
  // instance, like Overview, Add-ons and Worlds; the instance list, switching
  // and creating live in the sidebar.
  //
  // Not keyed on the instance id: a folder rename changes the id (the directory
  // name IS the id), and a remount would close Advanced, drop focus and tear down
  // a dialog or the tour mid-way. The sections seed their drafts per id instead.
  import { commands, type InstanceWithStatus, type VersionEntry } from '$lib/ipc/bindings';
  import { deleteBlockedReason } from '$lib/instances/instance-menu';
  import { instanceIpcErrorMessage } from '$lib/instances/instance-ipc-error';
  import IntegritySection from '$lib/instances/IntegritySection.svelte';
  import { displayLauncher } from '$lib/instances/launcher-display';
  import { shouldFocusField, type ManageFocusField } from '$lib/instances/manage-focus';
  import ContextualTour from '$lib/onboarding/ContextualTour.svelte';
  import { INSTANCE_SETTINGS_STEPS } from '$lib/onboarding/contextual-tours';
  import { t } from '$lib/i18n';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { serversUi } from '$lib/servers/servers-ui.svelte';
  import { taskFor } from '$lib/tasks/registry.svelte';
  import { pushWarning } from '$lib/toasts/toasts.svelte';
  import { fieldFlash } from '$lib/ui/field-flash';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import InstanceGameSection from './InstanceGameSection.svelte';
  import InstanceIdentitySection from './InstanceIdentitySection.svelte';
  import InstanceLaunchSection from './InstanceLaunchSection.svelte';
  import {
    clearInstanceSettingsNav,
    instanceSettingsNav,
    requestIsFor,
  } from './instance-settings-nav.svelte';

  let {
    instance,
    instances,
    instancesLoaded,
    listError,
    fellBack,
    versions,
    running,
    anyRunning,
    onChanged,
    onCreate,
    onDelete,
    onClone,
    onShortcut,
    onTranslations,
    onOpenFolder,
  }: {
    /** The active instance; null when there is none (or it could not be read). */
    instance: InstanceWithStatus | null;
    instances: InstanceWithStatus[];
    /** False until the first instance list arrived — "not loaded" is not "none". */
    instancesLoaded: boolean;
    /** The instance list could not be read. */
    listError: string | null;
    /** The data folder is unavailable (a recovery session). */
    fellBack: boolean;
    versions: VersionEntry[];
    /** THIS instance's game is up. */
    running: boolean;
    /** Verify/repair touch shared libraries and refuse while ANY game runs. */
    anyRunning: boolean;
    onChanged: () => void;
    onCreate: () => void;
    onDelete: (id: string) => void;
    onClone: (id: string) => void;
    onShortcut?: (id: string) => void;
    onTranslations: (id: string) => void;
    onOpenFolder: (id: string) => void;
  } = $props();

  // A task scoped to this instance (install, verify, mod install, pack update,
  // world migration). The backend's write gate does not see those; the tab
  // sits next to Install, so it has to.
  const busy = $derived(instance ? taskFor({ instanceId: instance.id }) !== null : false);

  // Delete's availability, the one rule the sidebar menu shares. An unread
  // list is not a one-instance list: no "last instance" claim then.
  const deleteBlock = $derived(
    deleteBlockedReason({
      running,
      busy,
      last: listError === null && instances.length <= 1,
    }),
  );
  const deleteReason = $derived(
    deleteBlock === 'running'
      ? $t('instance.manage.runningBlocked')
      : deleteBlock === 'busy'
        ? $t('instance.settings.busyBlocked')
        : deleteBlock === 'last'
          ? $t('instance.manage.cannotDeleteLast')
          : null,
  );

  // A request to show a field (an Overview card, an attention item, Rename).
  // Taken only when it is about this instance; MainTabs switched here for it.
  let focusField = $state<ManageFocusField | null>(null);
  $effect(() => {
    const req = instanceSettingsNav.value;
    if (!instance || !requestIsFor(req, instance.id)) return;
    focusField = req?.field ?? null;
    clearInstanceSettingsNav();
  });
  const delivered = () => (focusField = null);

  // Another instance opens at the top, as a fresh visit would; a refresh of the
  // same one keeps the place, and so does a folder rename (the folder name is
  // the id: Identity reports the new one before the refresh brings it in).
  // Before the DOM update, so a field the request flashes scrolls from the top.
  let scroller = $state<HTMLElement | null>(null);
  let scrollId: string | null = null;
  $effect.pre(() => {
    const id = instance?.id ?? null;
    if (id === scrollId) return;
    if (scrollId !== null && scroller) scroller.scrollTop = 0;
    scrollId = id;
  });

  // Two columns once the tab is wide enough: what you SET on the left, what
  // you INSPECT on the right. Measured on mount (no one-to-two jump) and by a
  // ResizeObserver; the threshold is the tab's own width, not the window's.
  const TWO_COLUMN_AT = 1100;
  let width = $state(0);
  const twoColumn = $derived(width >= TWO_COLUMN_AT);
  function observeWidth(node: HTMLElement) {
    width = node.clientWidth;
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => (width = node.clientWidth));
    ro.observe(node);
    return { destroy: () => ro.disconnect() };
  }

  async function openSourceFolder(id: string) {
    const result = await commands.openImportedSourceFolder(id);
    if (result.status === 'error')
      pushWarning($t('instance.manage.openSourceFolderFailed'), [
        instanceIpcErrorMessage(result.error),
      ]);
  }
</script>

<!-- The tab owns its scroll container (as AddonsTab does): it opens at the top,
     and the reserved gutter keeps the width — and the column split — from
     jumping when a scrollbar appears. -->
<div
  class="h-full overflow-y-auto [scrollbar-gutter:stable] p-4"
  data-testid="instance-settings-tab"
  bind:this={scroller}
  use:observeWidth
>
  {#if instance}
    <div class={twoColumn ? 'w-full' : 'mx-auto max-w-[720px]'}>
      <div class={twoColumn ? 'grid grid-cols-2 items-start gap-x-8' : 'flex flex-col'}>
        <div class="flex flex-col gap-4">
          <InstanceIdentitySection
            {instance}
            {running}
            {busy}
            {focusField}
            onFocusDelivered={delivered}
            {onChanged}
            onFolderRenamed={(id) => (scrollId = id)}
            {onClone}
            {onShortcut}
            {onTranslations}
            {onOpenFolder}
          />
          <InstanceGameSection
            {instance}
            {versions}
            {running}
            {busy}
            {focusField}
            onFocusDelivered={delivered}
            {onChanged}
          />
          <InstanceLaunchSection {instance} {focusField} onFocusDelivered={delivered} {onChanged} />
        </div>

        <div class="flex flex-col">
          {#if instance.imported_from}
            <div
              class="mb-3 flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs"
              data-testid="imported-provenance"
            >
              <Icon name="folderOpen" size={14} class="mt-0.5 shrink-0 text-muted" />
              <div class="min-w-0 flex-1">
                <div class="text-secondary">
                  {$t('instance.manage.importedFromLabel', {
                    launcher: displayLauncher(instance.imported_from.launcher),
                  })}
                </div>
                <div
                  class="truncate font-mono text-muted"
                  use:tooltip={{ text: instance.imported_from.source_path, whenOverflowing: true }}
                >
                  {instance.imported_from.source_path}
                </div>
              </div>
              <button
                type="button"
                class="btn-secondary btn-xs inline-flex shrink-0 items-center gap-1"
                onclick={() => instance && void openSourceFolder(instance.id)}
                data-testid="open-source-folder-btn"
              >
                <Icon name="folderOpen" size={12} />
                {$t('instance.manage.openSourceFolderBtn')}
              </button>
            </div>
          {/if}

          {#if instance.created_from_server}
            <div
              class="mb-3 flex items-start gap-2 rounded-md bg-subtle px-3 py-2 text-xs"
              data-testid="created-from-server-provenance"
            >
              <Icon name="server" size={14} class="mt-0.5 shrink-0 text-muted" />
              <div class="min-w-0 flex-1 text-secondary">
                {$t('instance.manage.fromServerLabel', {
                  name:
                    serverState.list.find((s) => s.id === instance?.created_from_server)?.name ??
                    instance.created_from_server,
                })}
              </div>
            </div>
          {/if}

          <!-- The data-sharing settings designed by «Design data sharing between
               instances» take their own section here, between Launch and Integrity. -->

          <div
            data-focus-field="integrity"
            use:fieldFlash={{
              active: focusField === 'integrity',
              focus: shouldFocusField('integrity'),
              onDelivered: delivered,
            }}
          >
            <IntegritySection
              instanceId={instance.id}
              isRunning={anyRunning}
              name={instance.name}
              status={instance.integrity}
              java={instance.java}
              requirementKey={JSON.stringify([
                instance.mc_version,
                instance.loader,
                instance.loader_version,
                instance.ready,
              ])}
            />
          </div>
        </div>
      </div>

      <section
        class="mt-6 flex flex-col gap-2 border-t border-border-subtle pt-4"
        data-tour-ctx="settings-danger"
      >
        <h3 class="text-sm font-medium text-danger">{$t('instance.settings.dangerTitle')}</h3>
        <button
          type="button"
          class="btn-danger btn-sm inline-flex items-center gap-1.5 self-start"
          disabled={deleteBlock !== null}
          aria-describedby={deleteReason ? 'settings-delete-reason' : undefined}
          data-testid="settings-delete-instance"
          onclick={() => instance && onDelete(instance.id)}
        >
          <Icon name="trash" size={14} />
          {$t('instance.manage.deleteBtn')}
        </button>
        {#if deleteReason}
          <p id="settings-delete-reason" class="text-xs text-secondary">{deleteReason}</p>
        {/if}
      </section>
    </div>
  {:else if !instancesLoaded}
    <div class="py-8 flex justify-center" data-testid="instance-settings-loading">
      <Spinner size="sm" delayMs={150} class="text-muted" />
    </div>
  {:else if fellBack}
    <p class="text-sm text-muted">{$t('sidebar.noInstancesFallback')}</p>
  {:else if listError}
    <p class="text-sm text-danger" role="alert">{listError}</p>
  {:else if instances.length === 0}
    <div class="flex flex-col items-start gap-3">
      <p class="text-sm text-muted">{$t('instance.settings.noInstances')}</p>
      <button type="button" class="btn-primary btn-sm" onclick={onCreate}>
        {$t('sidebar.createInstance')}
      </button>
    </div>
  {:else}
    <p class="text-sm text-muted">{$t('instance.settings.noActive')}</p>
  {/if}
</div>

{#if instance && instancesLoaded && serversUi.mode === 'client'}
  <!-- Outside the tab's body so an instance switch does not tear it down (a
       tour destroyed while showing is marked seen). Client mode only: in servers
       mode this panel is mounted but hidden, and a tour there would run unseen. -->
  <ContextualTour id="instanceSettings" steps={INSTANCE_SETTINGS_STEPS} />
{/if}
