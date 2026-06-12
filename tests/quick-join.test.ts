import { describe, expect, it } from 'vitest';
import { isValidServerAddress } from '$lib/worlds/quick-join';

describe('isValidServerAddress', () => {
  it('accepts a plain host', () => {
    expect(isValidServerAddress('mc.example.net')).toBe(true);
  });
  it('accepts host:port', () => {
    expect(isValidServerAddress('mc.example.net:25566')).toBe(true);
  });
  it('rejects empty / whitespace', () => {
    expect(isValidServerAddress('')).toBe(false);
    expect(isValidServerAddress('   ')).toBe(false);
    expect(isValidServerAddress('mc example.net')).toBe(false);
  });
  it('rejects a bad port', () => {
    expect(isValidServerAddress('host:abc')).toBe(false);
    expect(isValidServerAddress('host:99999')).toBe(false);
    expect(isValidServerAddress('host:')).toBe(false);
  });
  it('rejects missing host and multiple colons', () => {
    expect(isValidServerAddress(':25565')).toBe(false);
    expect(isValidServerAddress('a:b:c')).toBe(false);
  });
  it('rejects overlong addresses', () => {
    expect(isValidServerAddress('x'.repeat(300) + '.net')).toBe(false);
  });
  it('rejects C1 control characters', () => {
    expect(isValidServerAddress('mc.net')).toBe(false); // NEL (U+0085)
    expect(isValidServerAddress('mc.net')).toBe(false); // APC (U+009F)
  });
});
