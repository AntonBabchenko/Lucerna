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
  opts: ActionToastOptions = {},
): number {
  const id = nextId++;
  store.toasts = [
    ...store.toasts,
    { id, kind, title, lines, action, ...(opts.secondary ? { secondary: opts.secondary } : {}) },
  ];
  if (opts.ttlMs !== undefined) {
    timers.set(id, { remaining: opts.ttlMs, startedAt: 0, handle: null, paused: 0 });
    resumeToastTimer(id);
  }
  return id;
}

// Auto-hide countdowns of timed toasts, by id. `paused` counts reasons to wait
// (pointer over the toast, focus inside it) so leaving with the pointer while
// focus stays inside does not restart the countdown.
type Timer = {
  remaining: number;
  startedAt: number;
  handle: ReturnType<typeof setTimeout> | null;
  paused: number;
};
const timers = new Map<number, Timer>();

/** The pointer or focus is on the toast: stop its auto-hide countdown. */
export function pauseToastTimer(id: number): void {
  const timer = timers.get(id);
  if (!timer) return;
  timer.paused += 1;
  if (timer.handle !== null) {
    clearTimeout(timer.handle);
    timer.handle = null;
    timer.remaining = Math.max(0, timer.remaining - (Date.now() - timer.startedAt));
  }
}

/** The pointer or focus left: continue the countdown with the time that was left. */
export function resumeToastTimer(id: number): void {
  const timer = timers.get(id);
  if (!timer) return;
  timer.paused = Math.max(0, timer.paused - 1);
  if (timer.paused > 0 || timer.handle !== null) return;
  timer.startedAt = Date.now();
  timer.handle = setTimeout(() => dismiss(id), timer.remaining);
}

/** Remove a toast by id — the × button, or an auto-dismiss timer. */
export function dismiss(id: number): void {
  const timer = timers.get(id);
  if (timer?.handle) clearTimeout(timer.handle);
  timers.delete(id);
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
