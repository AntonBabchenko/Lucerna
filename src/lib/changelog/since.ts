// Which changelog versions a user has not seen yet, decided by file order —
// the parser preserves newest-first order, so no semver library is needed.
// This is the heart of the post-update "What's new" prompt: pure and
// dependency-free, unit-tested in isolation.
import type { Changelog, ChangelogVersion } from './types';

/**
 * Versions newer than `seen`, up to and including `current`, newest-first.
 * Returns [] when there is nothing meaningful to show.
 *
 * Indices are positions in the newest-first list (`-1` = absent):
 * - `seen == null`             → [] (first-ever launch / first launch after this
 *                                    feature ships — no retroactive prompt).
 * - `current` absent           → [] (a dev build ahead of CHANGELOG.md).
 * - `seen` absent, current in  → [current] (user jumped from a version no longer
 *                                    listed; still show what they landed on).
 * - both present               → entries.slice(ci, si); empty when ci >= si
 *                                    (same version, or a downgrade).
 */
export function changelogSince(
  entries: Changelog,
  current: string,
  seen: string | null,
): ChangelogVersion[] {
  if (seen === null) return [];
  const ci = entries.findIndex((x) => x.version === current);
  if (ci === -1) return [];
  const si = entries.findIndex((x) => x.version === seen);
  if (si === -1) return [entries[ci]];
  return ci < si ? entries.slice(ci, si) : [];
}

/**
 * True iff at least one version has a non-empty section. An empty `[Unreleased]`
 * alone must not trigger a prompt — matching `ChangelogPanel`'s own drop of
 * empty-section versions.
 */
export function hasRenderableEntry(list: ChangelogVersion[]): boolean {
  return list.some((x) => x.sections.length > 0);
}
