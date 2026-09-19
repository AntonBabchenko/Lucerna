<script lang="ts">
  // The Manage modal's left pane: the New-profile button, the name filter and
  // the profile rows with their right-click menu. Selection and every action
  // belong to the modal; this component renders and reports.
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import ActiveBadge from '$lib/instances/ActiveBadge.svelte';
  import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';
  import { displayLoader } from '$lib/instances/loader-display';
  import { t } from '$lib/i18n';
  import ContextMenu from '$lib/ui/cards/ContextMenu.svelte';
  import { Icon } from '$lib/ui/icons';
  import type { ContextMenuItem } from '$lib/ui/menu-item';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    instances,
    selectedId,
    activeId,
    width,
    dataRootBlockedReason,
    menuFor = () => [],
    onSelect,
    onCreate,
  }: {
    instances: InstanceWithStatus[];
    selectedId: string | null;
    activeId: string | null;
    width: number;
    /** Non-null while the data root is unavailable: New profile is disabled
     *  and the text explains why (§7 fallback gating). */
    dataRootBlockedReason: string | null;
    /** Items for a row's right-click menu. An empty list leaves the row
     *  without one (ContextMenu stays inert). */
    menuFor?: (instance: InstanceWithStatus) => ContextMenuItem[];
    onSelect: (id: string) => void;
    onCreate: () => void;
  } = $props();

  // The filter only surfaces once the list is long enough that scanning becomes
  // a chore. Display-only: it never changes the selection, so the detail pane
  // keeps showing the selected instance even when the list hides it.
  const FILTER_THRESHOLD = 8;
  let filterQuery = $state('');
  const filteredInstances = $derived(
    filterQuery.trim()
      ? instances.filter((i) => i.name.toLowerCase().includes(filterQuery.trim().toLowerCase()))
      : instances,
  );
</script>

<aside
  class="shrink-0 p-2 flex flex-col gap-2"
  style="width:{width}px"
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
    <button
      type="button"
      class="shrink-0 btn-primary btn-sm w-full"
      onclick={() => {
        filterQuery = '';
        onCreate();
      }}
    >
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
      <!-- The menu acts on ITS row and leaves the selection alone, like the
           log-file rows and the sidebar's profile dropdown: a right-click is
           not a click. -->
      <ContextMenu items={menuFor(i)} ariaLabel={$t('instance.menu.aria', { name: i.name })}>
        <button
          class="text-left px-2 py-1 rounded text-sm hover:bg-subtle"
          class:bg-accent-soft={i.id === selectedId}
          aria-current={i.id === selectedId}
          data-testid="manage-row-{i.id}"
          onclick={() => onSelect(i.id)}
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
            {#if i.id === activeId}
              <ActiveBadge />
            {/if}
          </div>
          <div class="text-xs text-muted truncate">
            {displayLoader(i.loader)} · {i.mc_version || $t('instance.manage.pickMc')}
          </div>
        </button>
      </ContextMenu>
    {/each}
    {#if filterQuery.trim() && filteredInstances.length === 0}
      <p class="text-xs text-muted px-2 py-1">{$t('instance.manage.filterNoMatches')}</p>
    {/if}
  </div>
</aside>
