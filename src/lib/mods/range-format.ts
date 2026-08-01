import type { Translate } from '$lib/i18n';
import type { RangeClause, RangeDescription } from '$lib/ipc/bindings';

/**
 * Render a declared version range in plain language.
 *
 * The range was already decomposed into typed clauses by the Rust side (see
 * `mods::range_describe`), which is the only place either grammar is parsed.
 * This function performs NO parsing: it maps variants onto localized strings.
 * That split is deliberate — a second parser here would be free to drift from
 * what the evaluator actually decides, and disagreeing about "what this range
 * means" is the bug class this whole path exists to avoid.
 *
 * When the backend could not decompose the range, its raw declared string is
 * returned verbatim. Quoting the mod is honest; inventing a phrase we cannot
 * justify is not.
 */
export function formatRange(t: Translate, d: RangeDescription): string {
  if (d.unparseable || d.alternatives.length === 0) return d.raw;
  return d.alternatives
    .map((clauses) => clauses.map((c) => formatClause(t, c)).reduce(joiner(t, 'and')))
    .reduce(joiner(t, 'or'));
}

/** True when the range constrains nothing — never word it as a requirement. */
export function isSoftRange(d: RangeDescription): boolean {
  return d.soft;
}

function joiner(t: Translate, kind: 'and' | 'or') {
  const key = kind === 'and' ? 'mods.range.joinAnd' : 'mods.range.joinOr';
  return (a: string, b: string) => t(key, { a, b });
}

function formatClause(t: Translate, c: RangeClause): string {
  switch (c.kind) {
    case 'any':
      return t('mods.range.any');
    case 'soft':
      return t('mods.range.soft', { version: c.version });
    case 'exact':
      return t('mods.range.exact', { version: c.version });
    case 'at_least':
      return t('mods.range.atLeast', { version: c.version });
    case 'above':
      return t('mods.range.above', { version: c.version });
    case 'at_most':
      return t('mods.range.atMost', { version: c.version });
    case 'below':
      return t('mods.range.below', { version: c.version });
    // A span is two bounds joined, so the four inclusivity combinations reuse
    // the single-bound phrasings instead of needing four more strings.
    case 'between':
      return t('mods.range.joinAnd', {
        a: c.low_inclusive
          ? t('mods.range.atLeast', { version: c.low })
          : t('mods.range.above', { version: c.low }),
        b: c.high_inclusive
          ? t('mods.range.atMost', { version: c.high })
          : t('mods.range.below', { version: c.high }),
      });
  }
}
