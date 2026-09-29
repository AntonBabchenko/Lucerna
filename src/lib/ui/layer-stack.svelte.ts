// Single owner of overlay layering. Every open modal, popover and contextual
// tour sits in ONE ordered stack; the layer opened last is on top. Four rules
// follow from that order and all four live here, so no caller can get one of
// them half right:
//   - Escape goes to the top layer only (the router below). Lower layers never
//     see it — one keypress closes one thing.
//   - A contextual tour is shown only while it is the top layer. Whatever the
//     user opens during a tour goes on top, so it is never painted under the
//     tour's dim; the tour comes back when it closes.
//   - A modal's focus trap yields only to a tour lying above it (`tourAbove`).
//   - An OS file drop reaches the app only while the topmost MODAL, if any,
//     takes drops (`modalBlocksFileDrops`); popovers and tours never count.
// Same model as React Aria's overlay stack and Radix's DismissableLayer.
//
// Tours are inserted directly above their HOST (the modal they belong to, or
// the page), not pushed on top: a tour whose surface mounts under an open
// dialog waits below it instead of painting over it.

import { getContext, setContext, untrack } from 'svelte';

export type LayerId = symbol;
export type LayerKind = 'modal' | 'popover' | 'tour';

interface Layer {
  readonly id: LayerId;
  readonly kind: LayerKind;
  /** What a tour sits on — a layer, or null for the page. Null for the rest. */
  readonly host: LayerId | null;
  readonly onEscape: () => void;
  /** A modal whose body takes OS file drops (the Modpacks modal). False for the rest. */
  readonly takesFileDrops: boolean;
}

// Replaced, never mutated. Reads inside the mutators go through `untrack`:
// they run inside effects (useLayer, a tour's onMount) and must not make those
// effects depend on the stack — an effect that re-ran on every stack change
// would release and re-push its layer, reordering the stack.
let layers = $state.raw<readonly Layer[]>([]);
const listeners = new Set<() => void>();
let routerInstalled = false;

function current(): readonly Layer[] {
  return untrack(() => layers);
}

function commit(next: readonly Layer[]): void {
  layers = next;
  syncRouter(next.length > 0);
  for (const fn of listeners) fn();
}

function onKeydown(e: KeyboardEvent): void {
  // An element-level handler that already consumed the key (Select, Menu, a
  // search field clearing its query) owns this press.
  if (e.key !== 'Escape' || e.defaultPrevented) return;
  const top = current().at(-1);
  if (!top) return;
  // Swallowed even when the top layer declines to close (a busy dialog): the
  // layer underneath must not close in its place.
  e.preventDefault();
  top.onEscape();
}

function syncRouter(wanted: boolean): void {
  if (wanted === routerInstalled || typeof window === 'undefined') return;
  if (wanted) window.addEventListener('keydown', onKeydown);
  else window.removeEventListener('keydown', onKeydown);
  routerInstalled = wanted;
}

function release(id: LayerId): void {
  const now = current();
  if (!now.some((l) => l.id === id)) return;
  // A tour never outlives its host: drop it with the host rather than leave an
  // entry that no longer sits on anything.
  commit(now.filter((l) => l.id !== id && l.host !== id));
}

export function newLayerId(label: string): LayerId {
  return Symbol(label);
}

/**
 * Push a modal or popover on top. Returns an idempotent release.
 * `takesFileDrops`: the modal's body takes OS file drops; only a modal's is read.
 */
export function pushLayer(
  id: LayerId,
  kind: 'modal' | 'popover',
  onEscape: () => void,
  { takesFileDrops = false }: { takesFileDrops?: boolean } = {},
): () => void {
  const now = current();
  if (!now.some((l) => l.id === id))
    commit([...now, { id, kind, host: null, onEscape, takesFileDrops }]);
  return () => release(id);
}

/**
 * Insert a tour directly above `host` (null = the page, i.e. the bottom).
 * Returns null — the caller DEFERS, un-toured this visit and not burned — when
 * a tour already lies above `host` (for the page: when any tour exists), or when
 * `host` is not in the stack. One synchronous check-and-insert, so two tours
 * mounting in the same flush cannot both get in.
 */
export function insertTour(
  id: LayerId,
  host: LayerId | null,
  onEscape: () => void,
): (() => void) | null {
  const now = current();
  const hostIndex = host === null ? -1 : now.findIndex((l) => l.id === host);
  if (host !== null && hostIndex === -1) return null;
  if (now.some((l, i) => l.kind === 'tour' && i > hostIndex)) return null;
  if (now.some((l) => l.id === id)) return null;
  const next = [...now];
  next.splice(hostIndex + 1, 0, { id, kind: 'tour', host, onEscape, takesFileDrops: false });
  commit(next);
  return () => release(id);
}

/** Whether `id` is the top layer. Reactive. */
export function isTopmost(id: LayerId): boolean {
  return layers.at(-1)?.id === id;
}

/** The top layer is a tour lying above `id`. False when `id` is not in the stack. Reactive. */
export function tourAbove(id: LayerId): boolean {
  const index = layers.findIndex((l) => l.id === id);
  const top = layers.length - 1;
  return index !== -1 && top > index && layers[top].kind === 'tour';
}

/** How many modals are open; 0 when none. Reactive. */
export function modalDepth(): number {
  return layers.filter((l) => l.kind === 'modal').length;
}

/**
 * True while the topmost MODAL takes no OS file drops — then nothing does: its
 * scrim covers every drop box under it (DESIGN.md §14), which must neither
 * light up behind it nor take the files where nobody can see. False with no
 * modal open, or when the topmost one takes drops (the Modpacks modal).
 * Popovers and tours never count. Over a modal they open within its screen,
 * and its scrim still covers everything under it; with no modal open the drop
 * box stays in sight — a popover is a small surface over the page, and a
 * tour's dim darkens it without taking the pointer. The window drop router
 * reads it as `modalOnTop`. Reactive.
 */
export function modalBlocksFileDrops(): boolean {
  const top = layers.findLast((l) => l.kind === 'modal');
  return top !== undefined && !top.takesFileDrops;
}

/** Plain-callback subscription for code outside components (trap-focus). */
export function onLayersChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/**
 * Register a popover while `isOpen()` is true. Call during component
 * initialisation. The effect's teardown releases the layer on every way out —
 * close, or the component unmounting while open.
 */
export function useLayer(
  kind: 'modal' | 'popover',
  isOpen: () => boolean,
  onEscape: () => void,
): void {
  const id = newLayerId(kind);
  $effect(() => {
    if (!isOpen()) return;
    return pushLayer(id, kind, onEscape);
  });
}

const HOST_KEY = Symbol('layer-host');

/** Make `id` the host of every contextual tour rendered inside this component. */
export function provideLayerHost(id: LayerId): void {
  setContext(HOST_KEY, id);
}

/** The layer this component is rendered inside; null on the page. Call at init. */
export function layerHost(): LayerId | null {
  return getContext<LayerId | undefined>(HOST_KEY) ?? null;
}

/** Test seam: drop every layer. Production code must not call it. */
export function __resetLayers(): void {
  commit([]);
}
