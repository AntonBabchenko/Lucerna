import { getCurrentWebview } from '@tauri-apps/api/webview';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { reportNotAdded, skipReason } from '$lib/layout/drop-report';
import { type DropContext, type DropRoute, planDrop } from '$lib/layout/drop-router';
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
 * have their own, and a drop landed both there and in the tab underneath.
 *
 * One plan decides a drag, and the same plan carries out its drop: `enter` (the only event before
 * the drop that carries the paths) asks `planDrop` which box the drag lights up, whether it adds
 * anything there and why the other files would stay behind (`dropPreview`) — so a box never
 * promises to add files it will not take; `drop` asks again with the dropped paths, routes what
 * fits to its box's rune and names every file left behind in a toast, with the reason. `leave`
 * covers a drag that left the window or was cancelled.
 */
export function listenForFileDrops(context: () => DropContext): () => void {
  const pending = getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    switch (payload.type) {
      case 'enter':
        preview(payload.paths, context());
        return;
      case 'over':
        // Carries no paths; what `enter` decided holds for the whole drag.
        return;
      case 'leave':
        dropPreview.value = null;
        return;
      case 'drop': {
        dropPreview.value = null;
        const plan = planDrop(payload.paths, context());
        if (plan.route !== null) deliver(plan.route);
        const tr = get(t);
        reportNotAdded(plan.skipped.map((s) => ({ path: s.path, reason: skipReason(tr, s.why) })));
        return;
      }
    }
  });
  return () => {
    void pending.then((un) => un());
  };
}

function preview(paths: string[], ctx: DropContext): void {
  // No paths: not a file drag — nothing to light up.
  if (paths.length === 0) {
    dropPreview.value = null;
    return;
  }
  const plan = planDrop(paths, ctx);
  const tr = get(t);
  dropPreview.value = {
    target: plan.host?.target ?? null,
    adds: plan.route !== null,
    notes: [...new Set(plan.skipped.map((s) => skipReason(tr, s.why)))],
  };
}

function deliver(route: NonNullable<DropRoute>): void {
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
