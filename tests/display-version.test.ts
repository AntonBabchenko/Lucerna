import { describe, expect, it } from 'vitest';
import { displayVersion } from '$lib/format/version';

// Plan §5b V1: «vmc1.21.1-0.13.1» — a «v» glued to a version that starts with a letter reads as
// part of it. The «v» goes only before a version that starts with a digit.
describe('displayVersion', () => {
  it('prefixes a version that starts with a digit', () => {
    expect(displayVersion('0.6.0+mc1.21.1')).toBe('v0.6.0+mc1.21.1');
    expect(displayVersion('13.0.121')).toBe('v13.0.121');
  });

  it('leaves any other version as it is', () => {
    expect(displayVersion('mc1.21.1-0.13.1')).toBe('mc1.21.1-0.13.1');
    expect(displayVersion('v2.1')).toBe('v2.1');
    expect(displayVersion('beta-3')).toBe('beta-3');
    // The unknown-version placeholder is no version either.
    expect(displayVersion('?')).toBe('?');
    expect(displayVersion('')).toBe('');
  });
});
