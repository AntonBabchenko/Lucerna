// Theme runtime: bridges the persisted user preference, the OS
// prefers-color-scheme media query, and the .dark class on <html>.
//
//   themeState.pref     — 'system' | 'light' | 'dark', the saved choice.
//   resolvedTheme()     — 'light' | 'dark', what the UI is actually
//                         displaying right now.
//
// FOUC prevention is handled by an inline script in app.html that
// runs before this module loads — so the html class is already set
// correctly at first paint. This module's job is the reactive
// updates from then on (settings panel changes, OS preference flips
// while pref === 'system').

import { commands, type ThemePreference } from '$lib/ipc/bindings';

const STORAGE_KEY = 'theme';

function readSystemPrefersDark(): boolean {
  if (typeof window === 'undefined') return false;
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

function resolve(pref: ThemePreference, systemDark: boolean): 'light' | 'dark' {
  if (pref === 'dark') return 'dark';
  if (pref === 'light') return 'light';
  return systemDark ? 'dark' : 'light';
}

function applyClass(resolved: 'light' | 'dark') {
  if (typeof document === 'undefined') return;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
}

export const themeState = $state<{
  pref: ThemePreference;
  systemDark: boolean;
}>({
  pref: 'system',
  systemDark: readSystemPrefersDark(),
});

export function resolvedTheme(): 'light' | 'dark' {
  return resolve(themeState.pref, themeState.systemDark);
}

/**
 * Called once at app start with the persisted pref from app.json.
 * Mirrors into localStorage so the next launch's FOUC script reads
 * the right value. Sets up the matchMedia listener for live OS
 * preference changes. Returns an unlisten function.
 */
export function initTheme(initial: ThemePreference): () => void {
  themeState.pref = initial;
  try {
    localStorage.setItem(STORAGE_KEY, initial);
  } catch (_) {
    /* private mode etc. — degrade silently */
  }
  applyClass(resolve(initial, themeState.systemDark));

  if (typeof window === 'undefined') return () => {};
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  const onChange = (e: MediaQueryListEvent) => {
    themeState.systemDark = e.matches;
    applyClass(resolve(themeState.pref, themeState.systemDark));
  };
  mq.addEventListener('change', onChange);
  return () => mq.removeEventListener('change', onChange);
}

// Serialize the general-settings read-modify-write so two rapid theme picks
// can't interleave their get→set windows and have the earlier write clobber the
// later one with a stale `general` snapshot. Each call chains after the prior
// one, so the read always sees the previous write's result.
let persistChain: Promise<boolean> = Promise.resolve(true);

/** Apply a preference to the three places the live UI reads it from. Shared by
 *  the user-driven set and its rollback so the two can never diverge. */
function applyPref(pref: ThemePreference): void {
  themeState.pref = pref;
  try {
    localStorage.setItem(STORAGE_KEY, pref);
  } catch (_) {
    /* ignore */
  }
  applyClass(resolve(pref, themeState.systemDark));
}

/** Persist `pref` into GeneralSettings. Returns false when the read-modify-write
 *  did not reach disk, so the caller can roll the UI back instead of leaving a
 *  theme on screen that the next launch will not restore. */
async function persistThemePref(pref: ThemePreference): Promise<boolean> {
  const get = await commands.appSettingsGet();
  if (get.status !== 'ok') return false;
  const next = { ...get.data.general, theme: pref };
  const res = await commands.appSettingsSetGeneral(next);
  return res.status === 'ok';
}

/**
 * Called from the Settings panel when the user picks a new theme.
 * Updates the rune, mirrors to localStorage, flips the html class,
 * and persists to app.json via app_settings_set_general. On a failed read OR
 * write everything rolls back — including the localStorage mirror, which is
 * what app.html's anti-FOUC script reads at the next launch. Same contract as
 * `setExplanationLevel` / `setHidden`.
 */
export async function setThemePref(pref: ThemePreference): Promise<void> {
  const prev = themeState.pref;
  applyPref(pref);
  // Run the persist RMW through the shared chain; swallow a prior failure so one
  // failed write doesn't wedge every subsequent theme change.
  persistChain = persistChain.catch(() => false).then(() => persistThemePref(pref));
  const persisted = await persistChain;
  // The chain serializes the WRITES, not the clicks: a newer pick may already be
  // on screen by the time this one resolves, and rolling back then would revert
  // it. Only the pick still displayed may be reverted — the same post-await
  // re-check LogsPopover.loadContent makes before every state commit.
  if (!persisted && themeState.pref === pref) applyPref(prev);
}
