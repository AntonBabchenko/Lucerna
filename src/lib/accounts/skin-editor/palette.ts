// Pure, persistence-free palette model for the skin editor. The colour strip is a
// single fully user-editable palette; these helpers own its shape and every
// mutation is immutable (returns a new array, never mutates the input). All
// swatches are opaque (a = 255) — the painting pipeline only consumes RGB. The
// reactive singleton in palette.svelte.ts wraps this with $state + localStorage.
import type { Rgba } from '$lib/accounts/skin-editor/buffer';

/** Seed palette and reset target: the editor's original 10 colours. */
export const DEFAULT_PALETTE: Rgba[] = [
  [224, 224, 224, 255],
  [60, 60, 60, 255],
  [176, 125, 86, 255], // skin tone
  [122, 82, 52, 255], // darker skin tone
  [70, 49, 31, 255], // hair brown
  [46, 122, 158, 255], // shirt blue
  [58, 74, 138, 255], // trouser blue
  [163, 45, 45, 255], // red
  [59, 109, 17, 255], // green
  [239, 159, 39, 255], // amber
];

/** Upper bound on palette length; the "+" control is disabled at this size. */
export const MAX_SWATCHES = 24;

const opaque = (c: Rgba): Rgba => [c[0], c[1], c[2], 255];

const clampIndex = (i: number, len: number): number => (i < 0 ? 0 : i >= len ? len - 1 : i);

export function addSwatch(list: Rgba[], colour: Rgba): Rgba[] {
  if (list.length >= MAX_SWATCHES) return [...list];
  return [...list, opaque(colour)];
}

export function removeSwatch(list: Rgba[], index: number): Rgba[] {
  if (index < 0 || index >= list.length) return [...list];
  return [...list.slice(0, index), ...list.slice(index + 1)];
}

export function replaceSwatch(list: Rgba[], index: number, colour: Rgba): Rgba[] {
  if (index < 0 || index >= list.length) return [...list];
  const out = [...list];
  out[index] = opaque(colour);
  return out;
}

export function moveSwatch(list: Rgba[], from: number, to: number): Rgba[] {
  if (list.length < 2) return [...list];
  const f = clampIndex(from, list.length);
  const t = clampIndex(to, list.length);
  if (f === t) return [...list];
  const out = [...list];
  const [moved] = out.splice(f, 1);
  out.splice(t, 0, moved);
  return out;
}

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

/** Parse a "#rrggbb" string into an opaque Rgba, or null if malformed. */
export function parseHexColour(raw: unknown): Rgba | null {
  if (typeof raw !== 'string' || !HEX_RE.test(raw)) return null;
  return [
    Number.parseInt(raw.slice(1, 3), 16),
    Number.parseInt(raw.slice(3, 5), 16),
    Number.parseInt(raw.slice(5, 7), 16),
    255,
  ];
}

const toHex2 = (n: number): string =>
  Math.max(0, Math.min(255, n | 0))
    .toString(16)
    .padStart(2, '0');

/** Serialize to "#rrggbb" strings for storage (alpha dropped — always opaque). */
export function serializePalette(list: Rgba[]): string[] {
  return list.map((c) => `#${toHex2(c[0])}${toHex2(c[1])}${toHex2(c[2])}`);
}

/**
 * Coerce persisted data into a valid palette. A non-array (missing / corrupt)
 * falls back to a fresh copy of DEFAULT_PALETTE; a valid array is mapped
 * hex → Rgba, dropping malformed entries and capping at MAX_SWATCHES. A valid
 * empty array stays empty — the user may have cleared the palette deliberately.
 */
export function normalizePalette(raw: unknown): Rgba[] {
  if (!Array.isArray(raw)) return DEFAULT_PALETTE.map(opaque);
  const out: Rgba[] = [];
  for (const entry of raw) {
    const c = parseHexColour(entry);
    if (c) out.push(c);
    if (out.length >= MAX_SWATCHES) break;
  }
  return out;
}
