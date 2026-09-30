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
    .map((clauses) => formatAlternative(t, clauses))
    .reduce((a, b) => t('mods.range.joinOr', { a, b }));
}

/** True when the range constrains nothing — never word it as a requirement. */
export function isSoftRange(d: RangeDescription): boolean {
  return d.soft;
}

// One alternative. A lone bound reads as itself («0.5.11 и новее»). Bounds taken together — a
// span, or several clauses one alternative requires at once — read as ONE range, each bound in
// its span phrasing: «от 0.5.11, но ниже 0.6». Gluing the lone phrasings with «и» doubled the
// conjunction («0.5.11 и новее и ниже 0.6», plan §5b V1).
function formatAlternative(t: Translate, clauses: RangeClause[]): string {
  const [only] = clauses;
  if (clauses.length === 1 && only && only.kind !== 'between') return formatClause(t, only);
  return joinParts(
    t,
    clauses.flatMap((c) => spanParts(t, c)),
  );
}

/** A part of a span, and which side of it — if either — it bounds. */
type SpanPart = { text: string; side: 'low' | 'high' | null };

// The parts of one range, in the order the mod wrote them. Next to a bound on the other side, a
// bound limits it — «от 0.5.11, но ниже 0.6»; listed with a bare comma, the two read as two facts
// side by side (plan §5c V3). Anything else is listed.
function joinParts(t: Translate, parts: SpanPart[]): string {
  const [first, ...rest] = parts;
  if (!first) return '';
  let text = first.text;
  let prev = first;
  for (const part of rest) {
    const limits = prev.side !== null && part.side !== null && prev.side !== part.side;
    text = t(limits ? 'mods.range.joinSpan' : 'mods.range.joinAnd', { a: text, b: part.text });
    prev = part;
  }
  return text;
}

// A clause as a part of a span: a bound in its span phrasing; anything else as it reads alone.
function spanParts(t: Translate, c: RangeClause): SpanPart[] {
  const low = (text: string): SpanPart => ({ text, side: 'low' });
  const high = (text: string): SpanPart => ({ text, side: 'high' });
  switch (c.kind) {
    case 'at_least':
      return [low(t('mods.range.from', { version: c.version }))];
    case 'above':
      return [low(t('mods.range.above', { version: c.version }))];
    case 'at_most':
      return [high(t('mods.range.upTo', { version: c.version }))];
    case 'below':
      return [high(t('mods.range.below', { version: c.version }))];
    case 'between':
      return [
        low(
          c.low_inclusive
            ? t('mods.range.from', { version: c.low })
            : t('mods.range.above', { version: c.low }),
        ),
        high(
          c.high_inclusive
            ? t('mods.range.upTo', { version: c.high })
            : t('mods.range.below', { version: c.high }),
        ),
      ];
    case 'any':
    case 'soft':
    case 'exact':
      return [{ text: formatClause(t, c), side: null }];
  }
}

// A clause read alone.
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
    case 'between':
      // A span always reads as one range (`formatAlternative`).
      return joinParts(t, spanParts(t, c));
  }
}
