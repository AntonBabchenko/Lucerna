<script lang="ts">
  import SkinEditorFooter from '$lib/accounts/SkinEditorFooter.svelte';
  import type { Layer } from '$lib/accounts/skin-editor/atlas';
  import type { Rgba } from '$lib/accounts/skin-editor/buffer';
  import type { ViewerBg } from '$lib/accounts/skin-editor/viewer-bg';

  // Mirrors SkinEditorModal: the four plain choices are bound, and the parent writes
  // `colour` itself too (the eyedropper). The bound values are rendered as text so a
  // test can read what the parent actually holds.
  let colour = $state<Rgba>([224, 224, 224, 255]);
  let brush = $state(1);
  let activeLayer = $state<Layer>('base');
  let bg = $state<ViewerBg>('neutral');
  const noop = () => {};
  const hex = (c: Rgba) =>
    `#${c
      .slice(0, 3)
      .map((v) => v.toString(16).padStart(2, '0'))
      .join('')}`;
</script>

<SkinEditorFooter
  bind:colour
  bind:brush
  bind:activeLayer
  bind:bg
  pose="default"
  onPose={noop}
  variant="classic"
  onVariant={noop}
  baseVisible={true}
  overlayVisible={true}
  onToggleBase={noop}
  onToggleOverlay={noop}
  busy={false}
  isMicrosoft={true}
  saveError={null}
  applied={false}
  onLoadPng={noop}
  onExportPng={noop}
  onSaveToLibrary={noop}
  onOpenLibrary={noop}
  onApply={noop}
/>
<output data-testid="bound">{hex(colour)}|{brush}|{activeLayer}|{bg}</output>
<button type="button" data-testid="pick-red" onclick={() => (colour = [255, 0, 0, 255])}
  >pick</button
>
