<!--
  The bulk bar every installed list renders as the first child of its list container
  (DESIGN.md §9): «Select all» · «{count} selected» · the actions the host passes · Clear.
  The host owns the selection (createListSelection) and the operations; this is presentational.

  Rules, kept from the mods list's bar: the action group exists only while something is
  selected; `busy` disables every action while any operation runs; `busyAction` spins only the
  running one; Clear is never gated (deselecting is local state and safe mid-operation); a
  disabled action carries its reason as a tooltip on a wrapping span that keeps a tab stop, so
  the reason is reachable by keyboard too (DESIGN.md §5).
-->
<script module lang="ts">
  export type BulkBarAction = {
    /** Also the value `busyAction` takes while this action runs. */
    id: string;
    /** Already localized. */
    label: string;
    /** `danger` → `.btn-ghost-danger` (a removal); otherwise `.btn-secondary`. */
    intent?: 'secondary' | 'danger';
    /** Off on top of the bar's `busy` — e.g. no selected row can take the action. */
    disabled?: boolean;
    /** Why it is off; shown as the wrapper's tooltip. */
    disabledReason?: string;
    testid?: string;
  };
</script>

<script lang="ts">
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import SelectAllCheckbox from '$lib/ui/SelectAllCheckbox.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  let {
    allSelected,
    indeterminate,
    selectedCount,
    busy,
    busyAction,
    hint,
    actions,
    onToggleAll,
    onAction,
    onClear,
  }: {
    allSelected: boolean;
    indeterminate: boolean;
    selectedCount: number;
    /** Aggregate gate — disables every action while any operation (bulk or sibling) runs. */
    busy: boolean;
    /** The action in flight — only that button shows a spinner. */
    busyAction: string | null;
    /** What to select for, shown while nothing is selected. Already localized. */
    hint: string;
    actions: readonly BulkBarAction[];
    onToggleAll: (checked: boolean) => void;
    onAction: (id: string) => void;
    onClear: () => void;
  } = $props();
</script>

<div class="flex items-center gap-3 px-3 py-2 border-b border-border-subtle bg-subtle/40 text-sm">
  <SelectAllCheckbox
    {allSelected}
    {indeterminate}
    onToggle={onToggleAll}
    testid="bulk-select-all"
  />
  {#if selectedCount > 0}
    <span class="font-medium text-accent"
      >{$t('ui.bulk.selectedCount', { count: selectedCount })}</span
    >
    <div data-testid="bulk-bar" class="ml-auto flex items-center gap-1">
      {#each actions as action (action.id)}
        {@const off = action.disabled === true}
        {@const reason = off && action.disabledReason ? action.disabledReason : ''}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <span
          class="inline-flex"
          tabindex={reason ? 0 : undefined}
          use:tooltip={{ text: reason, describe: false }}
        >
          <BusyButton
            busy={busyAction === action.id}
            disabled={busy || off}
            class={`${action.intent === 'danger' ? 'btn-ghost-danger' : 'btn-secondary'} btn-xs`}
            data-testid={action.testid}
            onclick={() => onAction(action.id)}>{action.label}</BusyButton
          >
        </span>
      {/each}
      <!-- Clear is deliberately not gated on `busy`: deselecting is a local-only state reset
           and is safe to do while a bulk IPC op is in flight. -->
      <button type="button" class="btn-ghost btn-xs" onclick={onClear}>{$t('ui.bulk.clear')}</button
      >
    </div>
  {:else}
    <span class="text-muted text-xs">{hint}</span>
  {/if}
</div>
