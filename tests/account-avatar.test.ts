import { describe, expect, test } from 'vitest';
import { deriveAccountAvatar } from '../src/lib/accounts/account-avatar';

describe('deriveAccountAvatar', () => {
  test('uses the uppercased first codepoint as the letter', () => {
    expect(deriveAccountAvatar('steve').letter).toBe('S');
    expect(deriveAccountAvatar('  alex').letter).toBe('A');
  });

  test('falls back to ? for an empty name', () => {
    expect(deriveAccountAvatar('').letter).toBe('?');
    expect(deriveAccountAvatar('   ').letter).toBe('?');
  });

  test('hue is deterministic for the same name', () => {
    expect(deriveAccountAvatar('Notch').hue).toBe(deriveAccountAvatar('Notch').hue);
  });

  test('hue is within [0, 360)', () => {
    const { hue } = deriveAccountAvatar('SomePlayer123');
    expect(hue).toBeGreaterThanOrEqual(0);
    expect(hue).toBeLessThan(360);
  });

  test('different names usually produce different hues', () => {
    expect(deriveAccountAvatar('AAA').hue).not.toBe(deriveAccountAvatar('zzz').hue);
  });
});
