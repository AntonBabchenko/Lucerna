import type { DepRoot } from '$lib/ipc/bindings';

// The relation cell of an installed mod's row (DESIGN.md §9, spec 2026-09-30): what its two slots
// show, and how wide the list's column is so that every row's cell lines up.

/** What a row knows about its mod's place in the dependency graph. */
export type RelationInput = {
  /** The platform could not describe the installed version: its requirements are unknown. */
  depsUnknown: boolean;
  /** Required dependencies by distinct project, through the subtree (`depCounts`). */
  depTotal: number;
  /** The root's own optional dependencies. */
  optionalTotal: number;
  /** Installed mods that require it. */
  requiredByCount: number;
};

/** The two slots: ⛓ its own dependencies, ↑ what requires it. Null = nothing in that slot. */
export type RelationFigures = { dep: string | null; by: string | null };

/** A row's input, read off its graph root the one way the row and the list both use. */
export function relationInput(
  root: DepRoot | undefined,
  depTotal: number,
  requiredByCount: number,
): RelationInput {
  return {
    depsUnknown: !!root?.deps_unknown,
    depTotal,
    optionalTotal: root?.optional.length ?? 0,
    requiredByCount,
  };
}

/**
 * ⛓ counts required dependencies by project — «?» while the platform could not describe them
 * (unknown is not zero) — and falls back to the optional count only when that is all the section
 * would show; ↑ counts the installed mods that require it. The cell's name and tooltip say the
 * same in words.
 */
export function relationFigures(i: RelationInput): RelationFigures {
  let dep: string | null = null;
  if (i.depsUnknown) dep = '?';
  else if (i.depTotal > 0) dep = String(i.depTotal);
  else if (i.requiredByCount === 0 && i.optionalTotal > 0) dep = String(i.optionalTotal);
  return { dep, by: i.requiredByCount > 0 ? String(i.requiredByCount) : null };
}

/** Whether the cell has anything to open. */
export const hasFigures = (f: RelationFigures): boolean => f.dep !== null || f.by !== null;

/**
 * The column's slot widths, in figures: the longest of each slot over every row of the profile,
 * one at least — so every row's cell is one width and the icons and names after it start at one
 * x, on every page and under every filter.
 */
export function relationSlotDigits(figures: Iterable<RelationFigures>): {
  dep: number;
  by: number;
} {
  let dep = 1;
  let by = 1;
  for (const f of figures) {
    dep = Math.max(dep, f.dep?.length ?? 0);
    by = Math.max(by, f.by?.length ?? 0);
  }
  return { dep, by };
}

/** The element id of a row's dependency section: its relation cell names it in `aria-controls`. */
export const depSectionId = (sha1: string): string => `dep-section-${sha1}`;
