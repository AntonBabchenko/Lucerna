<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { t } from '$lib/i18n';
  import { relativeDate } from '$lib/format/relative-time';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { modpackUpdates } from './modpack-updates.svelte';

  // One card in the Imported tab grid. Mirrors ModpackCard's shape so
  // the user sees a consistent grid across Browse and Imported. We
  // always show the package placeholder icon — fetching real pack icons for
  // imported instances would require another Modrinth/CurseForge call
  // per card on every render of the tab, which isn't worth it in v1.
  //
  // `created_unix_ms` is `number | null` on the bindings type (very old
  // instances created before that field existed never get it back-filled
  // in InstanceMeta::open). We coalesce nulls to empty-string in the
  // relative-time helper and skip the trailing "· imported …" segment.

  // `isModified` (bundle 2) flips the small amber "modified" tag in the
  // title row when the pack's installed mods have drifted from the
  // import-time snapshot (user added or removed mods). Computed by
  // ImportedView from modpack_status; defaults to false so callers
  // that don't track status (= every existing test, every old-style
  // call site) keep their current rendering.
  let {
    inst,
    onClick,
    isModified = false,
  }: { inst: InstanceWithStatus; onClick: () => void; isModified?: boolean } = $props();

  const updateEntry = $derived.by(() => {
    const s = modpackUpdates.statusFor(inst.id);
    return s?.kind === 'update_available' ? s.entry : null;
  });
</script>

<button
  type="button"
  class="text-left p-3 bg-surface border rounded hover:border-accent hover:shadow-sm transition-all w-full"
  onclick={onClick}
  data-testid="imported-card"
>
  <div class="flex gap-3">
    <div
      class="w-12 h-12 bg-subtle rounded flex items-center justify-center text-placeholder flex-shrink-0"
    >
      <Icon name="package" size={24} />
    </div>
    <div class="min-w-0 flex-1">
      <div class="font-semibold text-sm truncate">
        {inst.mrpack_name} v{inst.mrpack_version}
        {#if isModified}
          <span
            class="ml-2 text-[10px] font-medium px-1.5 py-0.5 rounded bg-warning-bg text-warning-text align-middle"
            use:tooltip={$t('modpacks.imported.card.modifiedTitle')}
            data-testid="imported-card-modified-tag"
          >
            {$t('modpacks.imported.card.modifiedTag')}
          </span>
        {/if}
        {#if updateEntry}
          <span
            class="ml-2 text-[10px] font-medium px-1.5 py-0.5 rounded bg-success-bg text-success align-middle"
            use:tooltip={$t('modpacks.imported.card.updateTitle')}
            data-testid="imported-card-update-tag"
          >
            {$t('modpacks.imported.card.updateTag', { version: updateEntry.version_number })}
          </span>
        {/if}
      </div>
      <div class="text-xs text-muted truncate">
        {$t('modpacks.imported.card.instanceLabel', { name: inst.name })}
      </div>
      <div class="text-xs text-placeholder mt-1 truncate">
        MC {inst.mc_version} · {displayLoader(inst.loader)}{inst.loader_version
          ? ' ' + inst.loader_version
          : ''}{inst.created_unix_ms != null
          ? ' · ' +
            $t('modpacks.imported.card.importedAt', {
              when: relativeDate($t, inst.created_unix_ms),
            })
          : ''}
      </div>
    </div>
  </div>
</button>
