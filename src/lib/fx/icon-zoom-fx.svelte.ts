// Frontend-only, persisted toggle for the decorative icon hover-zoom effect on
// icon-only buttons (.btn-icon / .btn-icon-sm). Cosmetic only — deliberately
// NOT part of the Rust GeneralSettings struct: there is no startup/FOUC need
// (unlike the theme), so keeping it client-side avoids a backend field +
// bindings regen. Persistence is an explicit setter (mirrors rainbowFx.set /
// setThemePref) rather than an $effect.root write-through, so it works outside
// a reactive context and is trivially testable.

const KEY = 'lucerna.fx.iconZoom';

export function loadIconZoomEnabled(): boolean {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === null) return true; // default: on
    return raw === 'true';
  } catch {
    return true; // localStorage unavailable — default on, non-fatal
  }
}

class IconZoomFx {
  enabled = $state<boolean>(loadIconZoomEnabled());

  set(value: boolean): void {
    this.enabled = value;
    try {
      localStorage.setItem(KEY, String(value));
    } catch {
      /* localStorage unavailable — non-fatal */
    }
  }
}

export const iconZoomFx = new IconZoomFx();
