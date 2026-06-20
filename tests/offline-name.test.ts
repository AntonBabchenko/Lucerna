import { describe, expect, it } from 'vitest';
import { offlineNameRejectionKey, validateOfflineName } from '$lib/accounts/offline-name';

describe('validateOfflineName', () => {
  it('accepts plain Latin names and boundary lengths', () => {
    expect(validateOfflineName('Steve')).toBeNull();
    expect(validateOfflineName('Alex_99')).toBeNull();
    expect(validateOfflineName('abc')).toBeNull();
    expect(validateOfflineName('abcdefghijklmnop')).toBeNull();
  });

  it('rejects too-short (incl. empty) and too-long', () => {
    expect(validateOfflineName('')).toBe('too_short');
    expect(validateOfflineName('ab')).toBe('too_short');
    expect(validateOfflineName('abcdefghijklmnopq')).toBe('too_long');
  });

  it('rejects Cyrillic, space, hyphen and dot as invalid_chars', () => {
    expect(validateOfflineName('Игрок')).toBe('invalid_chars');
    expect(validateOfflineName('a b c')).toBe('invalid_chars');
    expect(validateOfflineName('ab-cd')).toBe('invalid_chars');
    expect(validateOfflineName('ab.cd')).toBe('invalid_chars');
    expect(validateOfflineName('   ')).toBe('invalid_chars');
  });

  it('checks length before charset (long Cyrillic is too_long)', () => {
    expect(validateOfflineName('абвгдеёжзийклмноп')).toBe('too_long');
  });

  it('maps each reason to a distinct i18n key', () => {
    expect(offlineNameRejectionKey('too_short')).toBe('page.accounts.offlineNameTooShort');
    expect(offlineNameRejectionKey('too_long')).toBe('page.accounts.offlineNameTooLong');
    expect(offlineNameRejectionKey('invalid_chars')).toBe('page.accounts.offlineNameInvalidChars');
  });
});
