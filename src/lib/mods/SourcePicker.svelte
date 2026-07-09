<script lang="ts">
  import type { ModSource } from '$lib/ipc/bindings';
  import Select from '$lib/ui/Select.svelte';
  import { t } from '$lib/i18n';

  // Source is a context switch (which catalogue am I browsing), not a narrowing
  // filter — so it lives in the sub-tab header row, not the filter toolbar. Used
  // by both the Add-ons tab (Modrinth + CurseForge) and the Modpacks tab, which
  // passes allowFtb / allowAtlauncher to append FTB / ATLauncher.
  let {
    value,
    onChange,
    allowFtb = false,
    allowAtlauncher = false,
    options: sourceOptions = undefined,
  }: {
    value: ModSource;
    onChange: (value: ModSource) => void;
    allowFtb?: boolean;
    allowAtlauncher?: boolean;
    /**
     * Explicit source list, overriding the default modrinth/curseforge(+ftb/
     * +atlauncher) derivation entirely. Used by surfaces with a different
     * catalogue pairing (e.g. the Plugins tab: modrinth + hangar).
     */
    options?: ModSource[];
  } = $props();

  const SOURCE_LABEL: Record<ModSource, string> = {
    modrinth: 'Modrinth',
    curseforge: 'CurseForge',
    ftb: 'FTB',
    atlauncher: 'ATLauncher',
    hangar: 'Hangar',
  };

  const options = $derived(
    (
      sourceOptions ?? [
        'modrinth',
        'curseforge',
        ...(allowFtb ? (['ftb'] as const) : []),
        ...(allowAtlauncher ? (['atlauncher'] as const) : []),
      ]
    ).map((source) => ({ value: source, label: SOURCE_LABEL[source] })),
  );
</script>

<label class="text-sm text-secondary inline-flex items-center gap-1">
  {$t('mods.source.label')}
  <Select
    {value}
    {options}
    onChange={(v) => onChange(v as ModSource)}
    ariaLabel={$t('mods.source.ariaLabel')}
    class="filter-control"
    dataTestid="browse-source-select"
  />
</label>
