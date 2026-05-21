<script lang="ts">
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { onDestroy, onMount } from 'svelte';

  // Drag-and-drop / click dropzone for installing a mod from a local
  // .jar. Mirrors the modpack ImportDropzone: it only surfaces the
  // chosen paths — inspection, the compat dialog and the actual install
  // are owned by the parent (ModBrowserTab).
  //
  // `disabled` is set by the parent when there is no active instance, or
  // the instance is vanilla (no loader — mods do not apply).

  let {
    disabled = false,
    onPicked,
  }: {
    disabled?: boolean;
    onPicked: (paths: string[]) => void;
  } = $props();

  let dragOver = $state(false);
  let dropUnlisten: (() => void) | null = null;

  function jarsOnly(paths: string[]): string[] {
    return paths.filter((p) => p.toLowerCase().endsWith('.jar'));
  }

  onMount(async () => {
    dropUnlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (disabled) return;
      const t = event.payload.type;
      if (t === 'enter' || t === 'over') {
        dragOver = true;
      } else if (t === 'leave') {
        dragOver = false;
      } else if (t === 'drop') {
        dragOver = false;
        const jars = jarsOnly(event.payload.paths ?? []);
        if (jars.length > 0) onPicked(jars);
      }
    });
  });

  onDestroy(() => {
    dropUnlisten?.();
  });

  async function clickPicker() {
    if (disabled) return;
    const result = await openFile({
      multiple: true,
      filters: [{ name: 'Mod jar', extensions: ['jar'] }],
    });
    if (Array.isArray(result) && result.length > 0) onPicked(result);
  }
</script>

<div
  class="border-2 border-dashed rounded-lg p-4 text-center transition-colors"
  class:cursor-pointer={!disabled}
  class:border-blue-400={dragOver && !disabled}
  class:bg-blue-50={dragOver && !disabled}
  class:border-neutral-300={!dragOver || disabled}
  class:hover:border-blue-300={!disabled && !dragOver}
  class:opacity-50={disabled}
  onclick={clickPicker}
  onkeydown={(e) => {
    if (!disabled && (e.key === 'Enter' || e.key === ' ')) clickPicker();
  }}
  role="button"
  tabindex={disabled ? -1 : 0}
  aria-disabled={disabled}
  data-testid="mod-dropzone"
>
  {#if disabled}
    <div class="text-neutral-500 text-sm">Select a non-vanilla instance to install mods</div>
  {:else}
    <div class="text-neutral-700 text-sm">Drop a mod .jar here to install it</div>
    <div class="text-neutral-500 text-xs">or click to browse</div>
  {/if}
</div>
