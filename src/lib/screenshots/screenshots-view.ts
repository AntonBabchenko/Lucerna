// src/lib/screenshots/screenshots-view.ts
//
// Pure view logic shared by both screenshot surfaces: ordering, calendar
// grouping and group captions. No DOM, no IPC — unit-tested in isolation;
// ScreenshotBrowser renders its output. Mirrors journal-view.ts.

import type { Screenshot } from '$lib/ipc/bindings';

export type SortDir = 'newest' | 'oldest';

/**
 * A screenshot's timestamp as a plain number.
 *
 * specta exports every Rust `f64` as `number | null` (a non-finite float
 * serialises to `null`), so the nullability is a wire artefact rather than a
 * real state — a listed file has an mtime unless the filesystem refused one,
 * in which case 0 sorts it to the tail rather than hiding it.
 */
export function shotTime(shot: Screenshot): number {
  return shot.modified_unix_ms ?? 0;
}

/** Order by capture time. Returns a copy; the caller's array is never mutated. */
export function sortShots(shots: Screenshot[], dir: SortDir): Screenshot[] {
  const sign = dir === 'newest' ? -1 : 1;
  return [...shots].sort((a, b) => sign * (shotTime(a) - shotTime(b)));
}

export type Granularity = 'day' | 'month';

export interface ShotGroup {
  /** Stable, unique per group — used as the `{#each}` key. */
  key: string;
  /** Local midnight of the day, or the first of the month. */
  startMs: number;
  shots: Screenshot[];
}

/** Local calendar bucket a timestamp falls into. */
function bucketStart(ms: number, granularity: Granularity): number {
  const d = new Date(ms);
  return granularity === 'day'
    ? new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
    : new Date(d.getFullYear(), d.getMonth(), 1).getTime();
}

/**
 * Bucket shots into local calendar days or months, preserving the caller's
 * order both between and within groups. A single pass that accumulates the
 * current group — not a Map keyed by the rendered caption, which would fuse
 * groups whose captions happen to match.
 */
export function groupShots(shots: Screenshot[], granularity: Granularity): ShotGroup[] {
  const groups: ShotGroup[] = [];
  let current: ShotGroup | null = null;
  for (const shot of shots) {
    const startMs = bucketStart(shotTime(shot), granularity);
    if (!current || current.startMs !== startMs) {
      current = { key: `${granularity}:${startMs}`, startMs, shots: [] };
      groups.push(current);
    }
    current.shots.push(shot);
  }
  return groups;
}
