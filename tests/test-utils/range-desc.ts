/**
 * Builders for the `RangeDescription` the Rust side attaches to every
 * `DepViolation`. Tests that only care about a violation's shape use
 * `rawRangeDesc`, which renders as the declared string; tests that assert the
 * rendered sentence use `rangeDesc` with the clauses `mods::range_describe`
 * would actually produce.
 */

import type { RangeClause, RangeDescription, RangeFamily } from '$lib/ipc/bindings';

/** A decomposed range: one alternative made of the given clauses. */
export function rangeDesc(
  raw: string,
  clauses: RangeClause[],
  family: RangeFamily = 'maven',
): RangeDescription {
  return {
    raw,
    family,
    alternatives: [clauses],
    unparseable: false,
    soft: clauses.every((c) => c.kind === 'any' || c.kind === 'soft'),
  };
}

/**
 * An undecomposable range — the honest fallback, which renders as the raw
 * declared string.
 */
export function rawRangeDesc(raw: string, family: RangeFamily = 'maven'): RangeDescription {
  return { raw, family, alternatives: [], unparseable: true, soft: false };
}
