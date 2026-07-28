<script lang="ts">
  // Presentational saved-skins grid — no WebGL, no IPC. Parents own all
  // mutations; this renders tiles and fires callbacks so it stays testable
  // under jsdom. `manage` mode (SkinCapeModal) applies on click and carries a
  // per-tile menu; `pick` mode (editor's library picker) only picks.
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import OverflowMenu from '$lib/ui/OverflowMenu.svelte';
  import { Icon } from '$lib/ui/icons';
  import { drawSkinFront } from '$lib/accounts/skin-render';
  import type { CapeInfo, SkinLibraryItem } from '$lib/ipc/bindings';
  import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';

  let {
    entries,
    mode,
    busy = false,
    capes = [],
    onApply,
    onPick,
    onEdit,
    onDelete,
  }: {
    entries: SkinLibraryItem[];
    mode: 'manage' | 'pick';
    busy?: boolean;
    capes?: CapeInfo[];
    onApply?: (item: SkinLibraryItem) => void;
    onPick?: (item: SkinLibraryItem) => void;
    onEdit?: (item: SkinLibraryItem) => void;
    onDelete?: (item: SkinLibraryItem) => void;
  } = $props();

  let confirmDelete = $state<SkinLibraryItem | null>(null);

  // 2D front-body composite per tile (reuses the pure skin-render geometry —
  // no skinview3d). In jsdom the Image never fires onload; the canvas simply
  // stays blank, which keeps this component unit-testable.
  function renderTile(node: HTMLCanvasElement, item: SkinLibraryItem) {
    const draw = (it: SkinLibraryItem) => {
      const img = new Image();
      img.onload = () => drawSkinFront(node, img, it.variant, 3);
      img.src = `data:image/png;base64,${it.skin_png_base64}`;
    };
    draw(item);
    return { update: draw };
  }

  function capeName(id: string): string {
    const cape = capes.find((c) => c.id === id);
    return cape ? (cape.alias ?? cape.id) : id;
  }

  function menuFor(item: SkinLibraryItem): ContextMenuItem[] {
    return [
      {
        label: $t('skinLibrary.edit'),
        icon: 'edit',
        disabled: busy,
        onSelect: () => onEdit?.(item),
      },
      {
        label: $t('skinLibrary.delete'),
        icon: 'trash',
        danger: true,
        separatorBefore: true,
        disabled: busy,
        onSelect: () => (confirmDelete = item),
      },
    ];
  }
</script>

{#if entries.length === 0}
  <p class="text-xs text-muted">
    {mode === 'manage' ? $t('skinLibrary.empty') : $t('skinLibrary.emptyPick')}
  </p>
{:else}
  <div class="flex flex-wrap gap-2">
    {#each entries as item (item.id)}
      <div class="relative w-[96px]">
        <button
          type="button"
          class="w-full flex flex-col items-center gap-1 rounded-[10px] border border-border-subtle hover:border-border-emphasis p-1.5"
          onclick={() => (mode === 'pick' ? onPick?.(item) : onApply?.(item))}
          disabled={busy}
          aria-label={mode === 'pick'
            ? $t('skinLibrary.pick', { name: item.name })
            : $t('skinLibrary.apply', { name: item.name })}
        >
          <canvas use:renderTile={item} class="h-[64px]" style="image-rendering:pixelated"></canvas>
          <span class="text-xs text-secondary truncate w-full text-center">{item.name}</span>
          {#if mode === 'manage' && item.cape_id !== null}
            <span
              class="flex items-center gap-1 text-[10px] text-muted truncate w-full justify-center"
            >
              <Icon name="shirt" size={10} />
              {$t('skinLibrary.capeBadge', { name: capeName(item.cape_id) })}
            </span>
          {/if}
        </button>
        {#if mode === 'manage'}
          <div class="absolute top-1 right-1">
            <OverflowMenu
              items={menuFor(item)}
              ariaLabel={$t('skinLibrary.tileMenu', { name: item.name })}
            />
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

{#if confirmDelete}
  <ConfirmDialog
    title={$t('skinLibrary.deleteConfirmTitle')}
    bodyText={$t('skinLibrary.deleteConfirmBody', { name: confirmDelete.name })}
    confirmLabel={$t('skinLibrary.delete')}
    variant="danger"
    {busy}
    onCancel={() => (confirmDelete = null)}
    onConfirm={() => {
      const item = confirmDelete;
      confirmDelete = null;
      if (item) onDelete?.(item);
    }}
  />
{/if}
