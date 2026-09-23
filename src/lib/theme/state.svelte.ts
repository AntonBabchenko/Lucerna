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

import type { ThemePreference } from '$lib/ipc/bindings';
import { patchGeneral } from '$lib/settings/app-settings.svelte';

const STORAGE_KEY = 'theme';

/** The OS dark-mode preference, or null when it cannot be read. */
function readSystemPrefersDark(): boolean | null {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return null;
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

function resolve(pref: ThemePreference, systemDark: boolean | null): 'light' | 'dark' {
  if (pref === 'dark') return 'dark';
  if (pref === 'light') return 'light';
  // null = the OS preference could not be read. The painter has to paint
  // something, so it keeps the answer it always gave (light) — a guess, which
  // is why no label may present it as a fact.
  return systemDark === true ? 'dark' : 'light';
}

function applyClass(resolved: 'light' | 'dark') {
  if (typeof document === 'undefined') return;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
}

export const themeState = $state<{
  pref: ThemePreference;
  /** The OS preference; null when it could not be read. */
  systemDark: boolean | null;
}>({
  pref: 'system',
  systemDark: readSystemPrefersDark(),
});

export function resolvedTheme(): 'light' | 'dark' {
  return resolve(themeState.pref, themeState.systemDark);
}

/**
 * What "System" resolves to right now, or null when the OS preference could
 * not be read. resolvedTheme() must always answer — the painter has to paint
 * something — so it treats "unreadable" as light. A LABEL must not repeat that
 * guess as a fact: "System — currently light" would tell the user their OS is
 * light when Lucerna could not find out.
 */
export function resolvedThemeOrNull(): 'light' | 'dark' | null {
  if (themeState.pref !== 'system') return resolve(themeState.pref, themeState.systemDark);
  if (themeState.systemDark === null) return null;
  return themeState.systemDark ? 'dark' : 'light';
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

/**
 * Called from the Settings panel when the user picks a new theme.
 * Updates the rune, mirrors to localStorage, flips the html class,
 * and persists through the one settings contract (`patchGeneral`). When the
 * patch is refused or lost, everything rolls back — including the localStorage
 * mirror, which is what app.html's anti-FOUC script reads at the next launch —
 * and the panel says so next to the picker (`saveFailure('theme')`). Same
 * contract as `setExplanationLevel` / `setHidden`.
 */
export async function setThemePref(pref: ThemePreference): Promise<void> {
  const prev = themeState.pref;
  applyPref(pref);
  const r = await patchGeneral({ theme: pref });
  // The store serialises the WRITES, not the clicks: a newer pick may already be
  // on screen by the time this one resolves, and rolling back then would revert
  // it. Only the pick still displayed may be reverted — the same post-await
  // re-check LogsPopover.loadContent makes before every state commit.
  if (!r.ok && themeState.pref === pref) applyPref(prev);
}
