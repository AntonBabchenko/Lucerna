import { describe, expect, it } from 'vitest';
import { accentStripClass, type CardStatusKind, cardStatusStyle } from '$lib/ui/cards/card-status';

describe('cardStatusStyle', () => {
  const cases: Array<[CardStatusKind, { accent: string; badge: string; dim: boolean }]> = [
    ['none', { accent: 'none', badge: 'neutral', dim: false }],
    ['enabled', { accent: 'none', badge: 'success', dim: false }],
    ['disabled', { accent: 'muted', badge: 'muted', dim: true }],
    ['update', { accent: 'warning', badge: 'warning', dim: false }],
    ['from-pack', { accent: 'info', badge: 'info', dim: false }],
    ['cross-platform', { accent: 'none', badge: 'neutral', dim: false }],
    ['incompatible', { accent: 'danger', badge: 'danger', dim: false }],
    ['missing-deps', { accent: 'danger', badge: 'danger', dim: false }],
    ['distribution-disabled', { accent: 'warning', badge: 'warning', dim: false }],
    ['modified', { accent: 'warning', badge: 'warning', dim: false }],
    ['pack-update', { accent: 'success', badge: 'success', dim: false }],
  ];
  for (const [kind, expected] of cases) {
    it(`maps "${kind}"`, () => {
      expect(cardStatusStyle(kind)).toEqual(expected);
    });
  }
});

describe('accentStripClass', () => {
  it('maps tones to background utility classes', () => {
    expect(accentStripClass('success')).toBe('bg-success');
    expect(accentStripClass('muted')).toBe('bg-border-emphasis');
    expect(accentStripClass('warning')).toBe('bg-warning-text');
    expect(accentStripClass('info')).toBe('bg-accent');
    expect(accentStripClass('danger')).toBe('bg-danger');
    expect(accentStripClass('none')).toBe('bg-transparent');
  });
});
