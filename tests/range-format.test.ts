import { describe, expect, it } from 'vitest';
import type { Translate } from '$lib/i18n';
import en from '$lib/i18n/locales/en.json';
import type { RangeDescription } from '$lib/ipc/bindings';
import { formatRange } from '$lib/mods/range-format';

/**
 * Translate against the REAL shipped English strings rather than a copy, so a
 * reworded `mods.range.*` value can never silently diverge from what this test
 * claims the user sees.
 */
const t: Translate = (key, values) => {
  const tmpl = key
    .split('.')
    .reduce<unknown>((acc, k) => (acc as Record<string, unknown>)?.[k], en);
  if (typeof tmpl !== 'string') throw new Error(`missing i18n key: ${key}`);
  const v = values as Record<string, unknown> | undefined;
  return v ? tmpl.replace(/\{(\w+)\}/g, (_, k) => String(v[k] ?? `{${k}}`)) : tmpl;
};

function desc(partial: Partial<RangeDescription>): RangeDescription {
  return {
    raw: '',
    family: 'maven',
    alternatives: [],
    unparseable: false,
    soft: false,
    ...partial,
  };
}

describe('formatRange', () => {
  it('renders an upper bound without Maven bracket notation', () => {
    // The AsyncParticles range. "(,6.0.9]" is not something a player can read.
    const d = desc({
      raw: '(,6.0.9]',
      alternatives: [[{ kind: 'at_most', version: '6.0.9' }]],
    });
    expect(formatRange(t, d)).toBe('6.0.9 or older');
    expect(formatRange(t, d)).not.toContain('(');
  });

  it('renders a lower bound', () => {
    const d = desc({
      raw: '[1.3.51,)',
      alternatives: [[{ kind: 'at_least', version: '1.3.51' }]],
    });
    expect(formatRange(t, d)).toBe('1.3.51 or newer');
  });

  it('never words a soft range as a requirement', () => {
    const d = desc({
      raw: '1.21-1.3',
      soft: true,
      alternatives: [[{ kind: 'soft', version: '1.21-1.3' }]],
    });
    expect(formatRange(t, d)).toBe('1.21-1.3 recommended (any version works)');
  });

  it('builds a span out of its two bounds instead of a fifth phrasing', () => {
    const d = desc({
      raw: '[1.0,2.0)',
      alternatives: [
        [
          {
            kind: 'between',
            low: '1.0',
            low_inclusive: true,
            high: '2.0',
            high_inclusive: false,
          },
        ],
      ],
    });
    expect(formatRange(t, d)).toBe('1.0 or newer and older than 2.0');
  });

  it('joins AND terms within an alternative and OR across alternatives', () => {
    const d = desc({
      raw: '(,1.0],[1.2,)',
      alternatives: [[{ kind: 'at_most', version: '1.0' }], [{ kind: 'at_least', version: '1.2' }]],
    });
    expect(formatRange(t, d)).toBe('1.0 or older, or 1.2 or newer');

    const andTerms = desc({
      raw: '>=1.0.0 <2.0.0',
      family: 'fabric_predicate',
      alternatives: [
        [
          { kind: 'at_least', version: '1.0.0' },
          { kind: 'below', version: '2.0.0' },
        ],
      ],
    });
    expect(formatRange(t, andTerms)).toBe('1.0.0 or newer and older than 2.0.0');
  });

  it('falls back to the declared string when the range cannot be decomposed', () => {
    // Inventing a phrase we cannot justify is worse than quoting the mod.
    const d = desc({ raw: '1.2.x', family: 'fabric_predicate', unparseable: true });
    expect(formatRange(t, d)).toBe('1.2.x');
  });

  it('renders an empty range as "any version"', () => {
    const d = desc({ raw: '', soft: true, alternatives: [[{ kind: 'any' }]] });
    expect(formatRange(t, d)).toBe('any version');
  });
});
