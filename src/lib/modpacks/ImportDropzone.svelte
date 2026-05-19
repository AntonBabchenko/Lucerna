<script lang="ts">
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { onDestroy, onMount } from 'svelte';

  // Entry-point for the Modpack import flow: a big dashed dropzone the user
  // can either click (native file picker) or drag a .mrpack / .zip into.
  // The component only surfaces the chosen file path — manifest inspection,
  // picker dialog, and the actual install are owned by the parent
  // (ModpacksTab).
  //
  // Drag-drop uses Tauri's webview-level `onDragDropEvent` (not browser DnD
  // events) so the OS-native file path is available — the HTML5 File API
  // doesn't expose absolute paths the Rust backend needs.

  let {
    onPicked,
    onError,
  }: {
    onPicked: (path: string) => void;
    onError: (msg: string) => void;
  } = $props();

  let dragOver = $state(false);
  let dropUnlisten: (() => void) | null = null;

  function isAcceptedExtension(path: string): boolean {
    const lower = path.toLowerCase();
    return lower.endsWith('.mrpack') || lower.endsWith('.zip');
  }

  onMount(async () => {
    dropUnlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const t = event.payload.type;
      if (t === 'enter' || t === 'over') {
        dragOver = true;
      } else if (t === 'leave') {
        dragOver = false;
      } else if (t === 'drop') {
        dragOver = false;
        const path = event.payload.paths?.[0];
        if (!path) return;
        if (isAcceptedExtension(path)) {
          onPicked(path);
        } else {
          onError('Drop a .mrpack or .zip file.');
        }
      }
    });
  });

  onDestroy(() => {
    dropUnlisten?.();
  });

  async function clickPicker() {
    const result = await openFile({
      multiple: false,
      filters: [{ name: 'Modpack', extensions: ['mrpack', 'zip'] }],
    });
    if (typeof result === 'string') onPicked(result);
  }
</script>

<div
  class="border-2 border-dashed rounded-lg p-8 text-center cursor-pointer transition-colors"
  class:border-blue-400={dragOver}
  class:bg-blue-50={dragOver}
  class:border-neutral-300={!dragOver}
  class:hover:border-blue-300={!dragOver}
  onclick={clickPicker}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') clickPicker();
  }}
  role="button"
  tabindex="0"
  data-testid="import-dropzone"
>
  <div class="text-neutral-700 mb-2">Drop a .mrpack or .zip here</div>
  <div class="text-neutral-500 text-sm">or click to browse</div>
</div>
