// Reactive, persisted singleton for the skin editor's custom colour palette.
// Mirrors browser-prefs.svelte.ts: a module-lifetime $state + $effect.root
// write-through to localStorage. All mutation logic lives in the pure palette.ts;
// this holds only reactive state and persistence.
import type { Rgba } from '$lib/accounts/skin-editor/buffer';
import {
  addSwatch,
  DEFAULT_PALETTE,
  MAX_SWATCHES,
  moveSwatch,
  normalizePalette,
  removeSwatch,
  replaceSwatch,
  serializePalette,
} from '$lib/accounts/skin-editor/palette';

const KEY = 'lucerna.skinPalette';

const freshDefaults = (): Rgba[] => DEFAULT_PALETTE.map((c) => [...c] as Rgba);

export function loadPalette(): Rgba[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === null) return freshDefaults();
    return normalizePalette(JSON.parse(raw));
  } catch {
    return freshDefaults();
  }
}

class SkinPalette {
  swatches = $state<Rgba[]>(loadPalette());

  get isFull(): boolean {
    return this.swatches.length >= MAX_SWATCHES;
  }

  add(colour: Rgba): void {
    this.swatches = addSwatch(this.swatches, colour);
  }
  remove(index: number): void {
    this.swatches = removeSwatch(this.swatches, index);
  }
  replace(index: number, colour: Rgba): void {
    this.swatches = replaceSwatch(this.swatches, index, colour);
  }
  move(from: number, to: number): void {
    this.swatches = moveSwatch(this.swatches, from, to);
  }
  reset(): void {
    this.swatches = freshDefaults();
  }

  constructor() {
    try {
      // Module singleton for the whole app session — this $effect.root persists
      // intentionally and is never disposed (same reasoning as BrowserPrefs).
      $effect.root(() => {
        $effect(() => {
          try {
            localStorage.setItem(KEY, JSON.stringify(serializePalette(this.swatches)));
          } catch {
            /* localStorage unavailable — non-fatal */
          }
        });
      });
    } catch {
      /* $effect.root requires a reactive context; persistence is best-effort */
    }
  }
}

export const skinPalette = new SkinPalette();
