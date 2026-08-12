<!--
  The one select-all control (maintainer request 2026-08-12: five surfaces had
  three different patterns — a bare aria-labelled checkbox, a toggling button,
  and a select/clear button pair — with copy drifting between them).

  A labelled tri-state checkbox: checked = everything selected, indeterminate =
  a partial selection, unchecked = nothing. Unchecking clears, so no separate
  "deselect all" control is needed. Presentational only — the caller owns the
  selection model and passes the two booleans it already derives; scoping (e.g.
  "all *visible* rows" under an active filter) is therefore the caller's
  semantics, unchanged by this component.
-->
<script lang="ts">
  import { t } from '$lib/i18n';

  let {
    allSelected,
    indeterminate = false,
    disabled = false,
    onToggle,
    testid = 'select-all-checkbox',
  }: {
    allSelected: boolean;
    indeterminate?: boolean;
    disabled?: boolean;
    /** `checked` is the state the user asked for: true = select everything. */
    onToggle: (checked: boolean) => void;
    testid?: string;
  } = $props();
</script>

<label class="flex shrink-0 cursor-pointer select-none items-center gap-1.5 text-xs text-secondary">
  <input
    type="checkbox"
    {disabled}
    checked={allSelected}
    {indeterminate}
    onchange={(e) => onToggle((e.currentTarget as HTMLInputElement).checked)}
    data-testid={testid}
  />
  {$t('ui.selectAll')}
</label>
