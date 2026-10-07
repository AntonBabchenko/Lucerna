<script lang="ts">
  // The skin editor's footer, built from the shared controls so the editor reads like the
  // rest of the app (DESIGN.md §5, §6):
  //   row 1  drawing — colour + palette, brush, paint layer
  //   row 2  view    — pose, model, layer visibility, background
  //   row 3  actions — file and library icon buttons; Apply to account on the right
  // Split out of SkinEditorModal so it can be tested without the WebGL viewer. Every viewer
  // side effect (pose, model, visibility) stays in the modal behind a callback; the plain
  // choices (colour, brush, paint layer, background) are bound.
  import { tick } from 'svelte';
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import type { SkinVariant } from '$lib/ipc/bindings';
  import type { Rgba } from '$lib/accounts/skin-editor/buffer';
  import { skinPalette } from '$lib/accounts/skin-editor/palette.svelte';
  import { POSE_NAMES, type PoseName } from '$lib/accounts/skin-editor/poses';
  import {
    BG_CLASS,
    BG_LABEL,
    VIEWER_BGS,
    type ViewerBg,
  } from '$lib/accounts/skin-editor/viewer-bg';
  import ContextMenu, { type ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
  import { Icon, type IconName } from '$lib/ui/icons';
  import SegmentedControl, { type SegmentOption } from '$lib/ui/SegmentedControl.svelte';
  import ToggleChip from '$lib/ui/ToggleChip.svelte';
  import { tooltip } from '$lib/ui/tooltip';

  type PaintLayer = 'base' | 'overlay';

  let {
    colour = $bindable(),
    brush = $bindable(),
    activeLayer = $bindable(),
    bg = $bindable(),
    pose,
    onPose,
    variant,
    onVariant,
    baseVisible,
    overlayVisible,
    onToggleBase,
    onToggleOverlay,
    busy,
    isMicrosoft,
    saveError,
    applied,
    onLoadPng,
    onExportPng,
    onSaveToLibrary,
    onOpenLibrary,
    onApply,
  }: {
    colour: Rgba;
    brush: number;
    activeLayer: PaintLayer;
    bg: ViewerBg;
    pose: PoseName;
    onPose: (p: PoseName) => void;
    variant: SkinVariant;
    onVariant: (v: SkinVariant) => void;
    baseVisible: boolean;
    overlayVisible: boolean;
    onToggleBase: () => void;
    onToggleOverlay: () => void;
    busy: boolean;
    isMicrosoft: boolean;
    saveError: string | null;
    applied: boolean;
    onLoadPng: () => void;
    onExportPng: () => void;
    onSaveToLibrary: () => void;
    onOpenLibrary: () => void;
    onApply: () => void;
  } = $props();

  const rgbaToHex = (c: Rgba): string =>
    `#${c[0].toString(16).padStart(2, '0')}${c[1].toString(16).padStart(2, '0')}${c[2].toString(16).padStart(2, '0')}`;
  const hexToRgba = (hex: string): Rgba => [
    Number.parseInt(hex.slice(1, 3), 16),
    Number.parseInt(hex.slice(3, 5), 16),
    Number.parseInt(hex.slice(5, 7), 16),
    255,
  ];

  // --- custom palette --------------------------------------------------------
  let editIndex = $state<number | null>(null);
  let editColorInput: HTMLInputElement | null = null;
  let dragIndex: number | null = null;

  function addCurrentColour(): void {
    skinPalette.add(colour);
  }

  async function beginEditSwatch(i: number): Promise<void> {
    editIndex = i;
    await tick(); // let the hidden picker's value bind before we open it
    editColorInput?.click();
  }

  function onEditColour(hex: string): void {
    if (editIndex !== null) skinPalette.replace(editIndex, hexToRgba(hex));
    editIndex = null;
  }

  function resetPalette(): void {
    if (window.confirm($t('skinEditor.paletteResetConfirm'))) skinPalette.reset();
  }

  function swatchMenu(i: number): ContextMenuItem[] {
    return [
      { label: $t('skinEditor.paletteEdit'), icon: 'edit', onSelect: () => beginEditSwatch(i) },
      {
        label: $t('skinEditor.paletteMoveLeft'),
        icon: 'chevronLeft',
        disabled: i === 0,
        onSelect: () => skinPalette.move(i, i - 1),
      },
      {
        label: $t('skinEditor.paletteMoveRight'),
        icon: 'chevronRight',
        disabled: i === skinPalette.swatches.length - 1,
        onSelect: () => skinPalette.move(i, i + 1),
      },
      {
        label: $t('skinEditor.paletteRemove'),
        icon: 'trash',
        danger: true,
        separatorBefore: true,
        onSelect: () => skinPalette.remove(i),
      },
    ];
  }

  function onSwatchDrop(target: number): void {
    if (dragIndex !== null && dragIndex !== target) skinPalette.move(dragIndex, target);
    dragIndex = null;
  }

  // --- choices ---------------------------------------------------------------
  const BRUSHES = [
    { size: 1, labelKey: 'skinEditor.brushThin' },
    { size: 3, labelKey: 'skinEditor.brushMedium' },
    { size: 5, labelKey: 'skinEditor.brushThick' },
  ] as const satisfies ReadonlyArray<{ size: number; labelKey: TranslationKey }>;

  const POSE_LABEL: Record<PoseName, TranslationKey> = {
    default: 'skinEditor.poseDefault',
    tpose: 'skinEditor.poseTpose',
    walk: 'skinEditor.poseWalk',
    sit: 'skinEditor.poseSit',
  };

  const brushOptions = $derived(
    BRUSHES.map((b) => ({ value: String(b.size), label: $t(b.labelKey) })),
  );
  const layerOptions = $derived([
    { value: 'base', label: $t('skinEditor.layerBase') },
    { value: 'overlay', label: $t('skinEditor.layerOverlay') },
  ]);
  const poseOptions = $derived(POSE_NAMES.map((p) => ({ value: p, label: $t(POSE_LABEL[p]) })));
  const modelOptions = $derived([
    { value: 'classic', label: $t('cosmetics.modelClassic') },
    { value: 'slim', label: $t('cosmetics.modelSlim') },
  ]);
  const bgOptions = $derived(VIEWER_BGS.map((b) => ({ value: b, label: $t(BG_LABEL[b]) })));
</script>

<!-- The dot sits in an icon-sized box, so the three segments are as wide as an icon segment
     and a thin brush does not shrink its thumb to a sliver. -->
{#snippet brushDot(o: SegmentOption)}
  <span class="inline-flex h-4 w-4 items-center justify-center">
    <span
      class="rounded-full bg-current"
      style="width:{Number(o.value) * 2}px;height:{Number(o.value) * 2}px"
    ></span>
  </span>
{/snippet}

{#snippet bgSwatch(o: SegmentOption)}
  <span
    class="block h-4 w-4 rounded-sm border border-border-emphasis {BG_CLASS[o.value as ViewerBg]}"
  ></span>
{/snippet}

<!-- Disabled while busy, so the tooltip sits on a wrapper (DESIGN.md §5): a disabled button
     fires no pointer events. The button keeps the name, from the same key. -->
{#snippet action(icon: IconName, label: string, onclick: () => void)}
  <span class="inline-flex" use:tooltip={{ text: label, describe: false }}>
    <button type="button" class="btn-icon" aria-label={label} disabled={busy} {onclick}>
      <Icon name={icon} size={16} />
    </button>
  </span>
{/snippet}

<div class="flex flex-col gap-3 px-5 py-3 border-t border-border-subtle shrink-0">
  <!-- Row 1: drawing -->
  <div class="flex items-center gap-x-4 gap-y-2 flex-wrap">
    <div class="inline-flex items-center gap-1.5 flex-wrap">
      <span class="text-xs text-muted">{$t('skinEditor.colour')}</span>
      <span
        class="w-6 h-6 rounded border border-border-emphasis inline-block"
        style="background:{rgbaToHex(colour)}"
      ></span>
      <div class="flex items-center gap-1 flex-wrap">
        {#each skinPalette.swatches as swatch, i (i)}
          <ContextMenu items={swatchMenu(i)} ariaLabel={$t('skinEditor.paletteSwatchMenu')}>
            <button
              type="button"
              class="w-[18px] h-[18px] rounded border border-border-subtle {rgbaToHex(swatch) ===
              rgbaToHex(colour)
                ? 'outline outline-2 outline-accent -outline-offset-2'
                : ''}"
              style="background:{rgbaToHex(swatch)}"
              draggable="true"
              aria-label={rgbaToHex(swatch)}
              onclick={() => (colour = swatch)}
              ondragstart={() => (dragIndex = i)}
              ondragover={(e) => e.preventDefault()}
              ondrop={() => onSwatchDrop(i)}
            ></button>
          </ContextMenu>
        {/each}
        <button
          type="button"
          class="w-[18px] h-[18px] rounded border border-dashed border-border-emphasis inline-flex items-center justify-center text-muted disabled:opacity-40"
          disabled={skinPalette.isFull}
          aria-label={$t('skinEditor.paletteAdd')}
          use:tooltip={skinPalette.isFull
            ? $t('skinEditor.paletteFull')
            : $t('skinEditor.paletteAdd')}
          onclick={addCurrentColour}
        >
          <Icon name="plus" size={12} />
        </button>
        <button
          type="button"
          class="btn-icon btn-icon-sm"
          aria-label={$t('skinEditor.paletteReset')}
          use:tooltip={$t('skinEditor.paletteReset')}
          onclick={resetPalette}
        >
          <Icon name="refresh" size={14} />
        </button>
      </div>
      <label class="inline-flex items-center gap-1 text-xs text-secondary">
        <input
          type="color"
          value={rgbaToHex(colour)}
          oninput={(e) => (colour = hexToRgba(e.currentTarget.value))}
          aria-label={$t('skinEditor.customColour')}
          class="w-6 h-6 cursor-pointer border-0 bg-transparent p-0"
        />
      </label>
      <input
        type="color"
        class="sr-only"
        tabindex={-1}
        aria-hidden="true"
        bind:this={editColorInput}
        value={editIndex !== null ? rgbaToHex(skinPalette.swatches[editIndex]) : '#000000'}
        oninput={(e) => onEditColour(e.currentTarget.value)}
      />
    </div>
    <div class="inline-flex items-center gap-1.5">
      <span class="text-xs text-muted">{$t('skinEditor.brushSize')}</span>
      <SegmentedControl
        variant="boxed"
        ariaLabel={$t('skinEditor.brushSize')}
        options={brushOptions}
        value={String(brush)}
        onChange={(v) => (brush = Number(v))}
        content={brushDot}
      />
    </div>
    <div class="inline-flex items-center gap-1.5">
      <span class="text-xs text-muted">{$t('skinEditor.paintOn')}</span>
      <SegmentedControl
        variant="boxed"
        ariaLabel={$t('skinEditor.paintOn')}
        options={layerOptions}
        value={activeLayer}
        onChange={(v) => (activeLayer = v as PaintLayer)}
      />
    </div>
  </div>

  <!-- Row 2: view -->
  <div class="flex items-center gap-x-4 gap-y-2 flex-wrap">
    <div class="inline-flex items-center gap-1.5">
      <span class="text-xs text-muted">{$t('skinEditor.poseHeading')}</span>
      <SegmentedControl
        variant="boxed"
        ariaLabel={$t('skinEditor.poseHeading')}
        options={poseOptions}
        value={pose}
        onChange={(v) => onPose(v as PoseName)}
      />
    </div>
    <div class="inline-flex items-center gap-1.5">
      <span class="text-xs text-muted">{$t('skinEditor.model')}</span>
      <SegmentedControl
        variant="boxed"
        ariaLabel={$t('skinEditor.model')}
        options={modelOptions}
        value={variant}
        onChange={(v) => onVariant(v as SkinVariant)}
      />
    </div>
    <div class="inline-flex items-center gap-1.5">
      <span class="text-xs text-muted">{$t('skinEditor.layerVisibility')}</span>
      <div
        role="group"
        aria-label={$t('skinEditor.layerVisibility')}
        class="inline-flex items-center gap-1.5"
      >
        <ToggleChip
          active={baseVisible}
          tone="neutral"
          icon={baseVisible ? 'eye' : 'eyeOff'}
          label={$t('skinEditor.layerBase')}
          onToggle={onToggleBase}
        />
        <ToggleChip
          active={overlayVisible}
          tone="neutral"
          icon={overlayVisible ? 'eye' : 'eyeOff'}
          label={$t('skinEditor.layerOverlay')}
          onToggle={onToggleOverlay}
        />
      </div>
    </div>
    <div class="inline-flex items-center gap-1.5">
      <span class="text-xs text-muted">{$t('skinEditor.background')}</span>
      <SegmentedControl
        variant="boxed"
        ariaLabel={$t('skinEditor.background')}
        options={bgOptions}
        value={bg}
        onChange={(v) => (bg = v as ViewerBg)}
        content={bgSwatch}
      />
    </div>
  </div>

  <!-- Row 3: actions -->
  <div class="flex items-center gap-1 border-t border-border-subtle pt-3">
    {@render action('upload', $t('skinEditor.loadPng'), onLoadPng)}
    {@render action('download', $t('skinEditor.savePng'), onExportPng)}
    <span class="mx-1 h-4 w-px bg-border-subtle" aria-hidden="true"></span>
    {@render action('plus', $t('skinLibrary.editorSave'), onSaveToLibrary)}
    {@render action('gallery', $t('skinLibrary.editorLoad'), onOpenLibrary)}
    <div class="ml-auto flex items-center gap-2">
      {#if saveError}
        <span class="text-xs text-danger">{saveError}</span>
      {/if}
      {#if applied}
        <span class="text-xs text-success">{$t('skinEditor.applied')}</span>
      {/if}
      <span
        use:tooltip={isMicrosoft
          ? undefined
          : { text: $t('skinEditor.offlineHint'), describe: false }}
      >
        <button
          type="button"
          class="btn-primary btn-sm"
          onclick={onApply}
          disabled={busy || !isMicrosoft}
        >
          {$t('skinEditor.apply')}
        </button>
      </span>
    </div>
  </div>
</div>
