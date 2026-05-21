<script lang="ts">
  import type { ModpackHit } from '$lib/ipc/bindings';

  // One card in the modpack search grid (ModpackBrowseView). The card is
  // a button so the whole tile is clickable and the focus ring is
  // keyboard-reachable — clicking opens the version drawer, which is the
  // only action available on a search hit (no inline install).
  //
  // `downloads` is `number | null` on the bindings type (Modrinth omits
  // it on rare entries; CurseForge tail-section hits sometimes too), so
  // we coalesce to 0 the same way ModCard does for the mod browser.

  let { hit, onClick }: { hit: ModpackHit; onClick: () => void } = $props();
</script>

<button
  type="button"
  class="text-left p-3 bg-white border rounded hover:border-blue-300 hover:shadow-sm transition-all w-full"
  onclick={onClick}
  data-testid="modpack-card"
>
  <div class="flex gap-3">
    {#if hit.icon_url}
      <img src={hit.icon_url} alt="" class="w-12 h-12 rounded object-cover flex-shrink-0" />
    {:else}
      <div
        class="w-12 h-12 bg-neutral-100 rounded flex items-center justify-center text-neutral-400 flex-shrink-0"
      >
        📦
      </div>
    {/if}
    <div class="min-w-0 flex-1">
      <div class="font-semibold text-sm truncate">{hit.title}</div>
      <div class="text-xs text-neutral-500 line-clamp-2">{hit.description}</div>
      <div class="text-xs text-neutral-400 mt-1">
        {(hit.downloads ?? 0).toLocaleString()} downloads
      </div>
      {#if hit.distribution_allowed === false}
        <div
          class="mt-1 inline-block text-xs px-1.5 py-0.5 rounded bg-amber-100 text-amber-800"
        >
          CurseForge download disabled
        </div>
      {/if}
    </div>
  </div>
</button>
