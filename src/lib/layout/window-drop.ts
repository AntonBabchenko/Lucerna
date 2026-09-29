import { getCurrentWebview } from '@tauri-apps/api/webview';
import { type DropContext, dropHost, routeDrop } from '$lib/layout/drop-router';
import {
  dropPreview,
  droppedAssets,
  droppedModpack,
  droppedMods,
  droppedServer,
  droppedServerContent,
  droppedWorld,
} from '$lib/settings/state.svelte';

/**
 * The app's single window-level drag-drop listener (DESIGN.md §14), mounted once by +page.svelte
 * with its live `context`. Tauri hands the event to every listener of the window, so a second
 * listener would take the same drop again — the Modpacks modal and the server-import view used to
 * have their own, and a drop landed both there and in the tab underneath. Here the router decides
 * once: on `enter` it names the drop box the drag lights up (`dropPreview`), on `drop` it routes
 * the files to that box's rune. `leave` covers a drag that left the window or was cancelled.
 */
export function listenForFileDrops(context: () => DropContext): () => void {
  const pending = getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    switch (payload.type) {
      case 'enter':
        // No paths: not a file drag — nothing to light up.
        dropPreview.value =
          payload.paths.length === 0 ? null : { target: dropHost(context())?.target ?? null };
        return;
      case 'over':
        // Carries no paths; what `enter` decided holds for the whole drag.
        return;
      case 'leave':
        dropPreview.value = null;
        return;
      case 'drop':
        dropPreview.value = null;
        deliver(payload.paths, context());
        return;
    }
  });
  return () => {
    void pending.then((un) => un());
  };
}

function deliver(paths: string[], ctx: DropContext): void {
  const route = routeDrop(paths, ctx);
  if (route === null) return;
  switch (route.target) {
    case 'modpack':
      droppedModpack.value = route.path;
      return;
    case 'server-import':
      droppedServer.value = [route.path];
      return;
    case 'client-world':
      droppedWorld.value = route.paths;
      return;
    case 'client-mods':
      droppedMods.value = route.paths;
      return;
    case 'client-assets':
      droppedAssets.value = { kind: route.kind, paths: route.paths };
      return;
    case 'server-content':
      droppedServerContent.value = { kind: route.kind, paths: route.paths };
      return;
  }
}
