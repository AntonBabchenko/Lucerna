<script lang="ts">
  import type { InstanceWithStatus } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

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

  function relativeTime(ms: number | null): string {
    if (ms == null) return '';
    const diff = Date.now() - ms;
    const day = 1000 * 60 * 60 * 24;
    if (diff < day) return 'today';
    if (diff < 2 * day) return 'yesterday';
    const days = Math.floor(diff / day);
    if (days < 30) return `${days}d ago`;
    const months = Math.floor(days / 30);
    if (months < 12) return `${months}mo ago`;
    return `${Math.floor(months / 12)}y ago`;
  }
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
            title={$t('modpacks.imported.card.modifiedTitle')}
            data-testid="imported-card-modified-tag"
          >
            {$t('modpacks.imported.card.modifiedTag')}
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
            $t('modpacks.imported.card.importedAt', { when: relativeTime(inst.created_unix_ms) })
          : ''}
      </div>
    </div>
  </div>
</button>
