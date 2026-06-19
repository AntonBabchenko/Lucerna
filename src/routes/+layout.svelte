<script lang="ts">
  import '../app.css';
  import { TooltipLayer } from '$lib/ui/tooltip';
  import { iconZoomFx } from '$lib/fx/icon-zoom-fx.svelte';
  let { children } = $props();

  // Gate the decorative icon hover-zoom at the document root so a single
  // class toggle drives every .btn-icon / .btn-icon-sm. Frontend-only,
  // mirrors the rainbowFx preference (which is gated per-icon in the
  // Sidebar). prefers-reduced-motion zeroes the transform in app.css, so
  // reduced-motion users never see it regardless of this class.
  $effect(() => {
    document.documentElement.classList.toggle('fx-icon-zoom', iconZoomFx.enabled);
  });

  // Block WebView2's native right-click menu (Back / Refresh / Save-as /
  // Print / Inspect). It betrays the browser nature of the app and shows
  // entries that make no sense in a Minecraft launcher. F12 and
  // Ctrl+Shift+I still open devtools in `pnpm tauri dev`.
  //
  // EXCEPTION: editable form controls (input / textarea / contenteditable)
  // keep their native Cut / Copy / Paste menu — paste-heavy fields like the
  // CurseForge API key entry expect right-click → Paste. Mirrors the
  // .selectable opt-in philosophy used for text — suppress everywhere
  // except where the menu actually helps.
  function onContextMenu(e: MouseEvent) {
    // e.target may be Window (event dispatched directly on window) or any
    // non-Element node, so guard with `closest in target` before calling it.
    const target = e.target;
    if (
      target instanceof HTMLElement &&
      target.closest('input, textarea, [contenteditable="true"]')
    ) {
      return;
    }
    e.preventDefault();
  }
</script>

<svelte:window oncontextmenu={onContextMenu} />

{@render children?.()}

<TooltipLayer />
