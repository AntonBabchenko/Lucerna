<script lang="ts">
  import type { ModSource } from '$lib/ipc/bindings';
  import Select from '$lib/ui/Select.svelte';
  import { t } from '$lib/i18n';

  let { value, onChange }: { value: ModSource; onChange: (value: ModSource) => void } = $props();

  const options = [
    { value: 'modrinth', label: 'Modrinth' },
    { value: 'curseforge', label: 'CurseForge' },
  ];
</script>

<label class="text-sm text-secondary inline-flex items-center gap-1">
  {$t('mods.source.label')}
  <Select
    {value}
    {options}
    onChange={(v) => onChange(v as ModSource)}
    ariaLabel={$t('mods.source.ariaLabel')}
    class="filter-control"
  />
</label>
