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
  /** Optional second, quieter action after the first (e.g. "Skip this
   *  version"). The × only ever closes the toast — anything that should
   *  persist is an action the user can read before choosing it. */
  secondary?: ToastAction;
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

export type ActionToastOptions = {
  /** A second, quieter action. */
  secondary?: ToastAction;
  /** Auto-hide after this long. The countdown pauses while the toast is
   *  hovered or holds focus, and resumes with the time that was left. */
  ttlMs?: number;
};

/** Show a toast (any kind) with an action button; sticky unless `ttlMs` is set. */
export function pushActionToast(
  kind: ToastKind,
  title: string,
  action: ToastAction,
  lines: string[] = [],
  _opts: ActionToastOptions = {},
): number {
  // STUB (red): the options are accepted but ignored.
  const id = nextId++;
  store.toasts = [...store.toasts, { id, kind, title, lines, action }];
  return id;
}

/** The pointer or focus is on the toast: stop its auto-hide countdown. */
export function pauseToastTimer(_id: number): void {
  // STUB (red).
}

/** The pointer and focus left: continue the countdown with the time left. */
export function resumeToastTimer(_id: number): void {
  // STUB (red).
}

/** Remove a toast by id — the × button, or the success auto-dismiss timer. */
export function dismiss(id: number): void {
  store.toasts = store.toasts.filter((t) => t.id !== id);
}

/** Patch a live toast in place. The only way to turn a sticky progress toast
 *  into its own outcome toast without losing its slot — dismiss + re-push
 *  appends a new card at the bottom of the stack with a new id.
 *  Unknown id is a silent no-op, matching `updateToastProgress`. */
export function updateToast(
  id: number,
  patch: Partial<Pick<Toast, 'title' | 'kind' | 'lines' | 'action'>>,
): void {
  store.toasts = store.toasts.map((t) => (t.id === id ? { ...t, ...patch } : t));
}
