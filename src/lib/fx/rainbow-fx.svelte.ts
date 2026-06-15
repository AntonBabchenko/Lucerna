// Frontend-only, persisted toggle for the decorative rainbow hover effect on
// the sidebar "Browse modpacks" icon. Cosmetic only — deliberately NOT part of
// the Rust GeneralSettings struct: there is no startup/FOUC need (unlike the
// theme), so keeping it client-side avoids a backend field + bindings regen.
// Persistence is an explicit setter (mirrors setThemePref) rather than an
// $effect.root write-through, so it works outside a reactive context and is
// trivially testable.

const KEY = 'lucerna.fx.rainbowIcons';

export function loadRainbowEnabled(): boolean {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === null) return true; // default: on
    return raw === 'true';
  } catch {
    return true; // localStorage unavailable — default on, non-fatal
  }
}

class RainbowFx {
  enabled = $state<boolean>(loadRainbowEnabled());

  set(value: boolean): void {
    this.enabled = value;
    try {
      localStorage.setItem(KEY, String(value));
    } catch {
      /* localStorage unavailable — non-fatal */
    }
  }
}

export const rainbowFx = new RainbowFx();
