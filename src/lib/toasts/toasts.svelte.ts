// Shared toast-notification store. A success toast confirms a completed
// download/install and auto-dismisses; a warning toast reports a failure
// (with a detail list) and stays until the user closes it; an info toast
// surfaces a neutral informational state (blue accent) and stays until the
// user closes it.
//
// Rune-state-in-a-.svelte.ts module — the same idiom as
// `$lib/settings/state.svelte` and `$lib/onboarding/state.svelte`.

export type ToastKind = 'success' | 'warning' | 'info';

export type ToastAction = { label: string; run: () => void };

export type Toast = {
  id: number;
  kind: ToastKind;
  title: string;
  /** Detail lines; empty for a plain success toast. */
  lines: string[];
  /** Optional action button (e.g. "Update" on an update-available toast). */
  action?: ToastAction;
  /** Optional callback fired when the toast is dismissed via the × button —
   *  e.g. persisting a per-version "don't nag again" flag. */
  onDismiss?: () => void;
  /** Download/verify progress for a progress toast. `undefined` = not a
   *  progress toast (no bar). `null` = indeterminate (bar shown, unknown
   *  total). `0..1` = fraction complete. */
  progress?: number | null;
};

/** A success toast auto-dismisses this many milliseconds after it appears. */
export const SUCCESS_TTL_MS = 7000;

let nextId = 1;

// The $state lives on an object field so `dismiss` / `push*` can reassign
// `store.toasts` with a fresh array and every reader picks up the change.
const store = $state<{ toasts: Toast[] }>({ toasts: [] });

/** The current list of active toasts (reactive). */
export function toastList(): Toast[] {
  return store.toasts;
}

/** Show a green success toast; it auto-dismisses after `SUCCESS_TTL_MS`. */
export function pushSuccess(title: string, lines: string[] = []): number {
  const id = nextId++;
  store.toasts = [...store.toasts, { id, kind: 'success', title, lines }];
  setTimeout(() => dismiss(id), SUCCESS_TTL_MS);
  return id;
}

/** Show an amber warning toast; it stays until `dismiss` is called. */
export function pushWarning(title: string, lines: string[] = []): number {
  const id = nextId++;
  store.toasts = [...store.toasts, { id, kind: 'warning', title, lines }];
  return id;
}

/** Show a blue info toast for neutral informational states; stays until `dismiss` is called. */
export function pushInfo(title: string, lines: string[] = []): number {
  const id = nextId++;
  store.toasts = [...store.toasts, { id, kind: 'info', title, lines }];
  return id;
}

/** Show a blue info toast carrying a progress bar; starts indeterminate
 *  (`progress: null`). Stays until `dismiss` is called. */
export function pushProgress(title: string, lines: string[] = []): number {
  const id = nextId++;
  store.toasts = [...store.toasts, { id, kind: 'info', title, lines, progress: null }];
  return id;
}

/** Update the progress of a progress toast. `null` = indeterminate,
 *  `0..1` = fraction. A no-op if `id` is not an active toast. */
export function updateToastProgress(id: number, progress: number | null): void {
  store.toasts = store.toasts.map((t) => (t.id === id ? { ...t, progress } : t));
}

/** Show a sticky toast (any kind) with an action button. */
export function pushActionToast(
  kind: ToastKind,
  title: string,
  action: ToastAction,
  lines: string[] = [],
  onDismiss?: () => void,
): number {
  const id = nextId++;
  store.toasts = [...store.toasts, { id, kind, title, lines, action, onDismiss }];
  return id;
}

/** Remove a toast by id — the × button, or the success auto-dismiss timer. */
export function dismiss(id: number): void {
  store.toasts = store.toasts.filter((t) => t.id !== id);
}
