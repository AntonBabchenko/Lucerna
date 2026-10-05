<!--
  The row checkbox of every bulk-capable list — the first child of the row, before the icon
  (DESIGN.md §9). One label for every kind of row: «Select {name}». A click on it never
  bubbles, so a row whose body opens a detail view does not open it.
-->
<script lang="ts">
  import { t } from '$lib/i18n';

  let {
    checked,
    name,
    disabled = false,
    onChange,
    testid = undefined,
  }: {
    checked: boolean;
    /** What the row shows as its title — the project name, else the file name. */
    name: string;
    disabled?: boolean;
    onChange: (checked: boolean) => void;
    testid?: string;
  } = $props();
</script>

<input
  type="checkbox"
  class="flex-shrink-0"
  {checked}
  {disabled}
  aria-label={$t('ui.bulk.selectRow', { name })}
  data-testid={testid}
  onclick={(e) => e.stopPropagation()}
  onchange={(e) => onChange((e.currentTarget as HTMLInputElement).checked)}
/>
