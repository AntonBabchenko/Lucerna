import { describe, expect, it } from 'vitest';
import { defaultCloneName, INSTANCE_NAME_MAX } from '$lib/instances/clone-request';

describe('defaultCloneName', () => {
  it('appends the suffix to a short name', () => {
    expect(defaultCloneName('My Pack', ' (copy)')).toBe('My Pack (copy)');
  });

  it('truncates the base so a max-length name still fits the suffix', () => {
    const long = 'a'.repeat(INSTANCE_NAME_MAX);
    const out = defaultCloneName(long, ' (copy)');
    expect(out.length).toBeLessThanOrEqual(INSTANCE_NAME_MAX);
    expect(out.endsWith(' (copy)')).toBe(true);
  });

  it('trims a trailing space left by the cut', () => {
    // 25 chars of room with a 7-char suffix; the cut lands after "Pack " —
    // the dangling space must not survive between base and suffix.
    const name = 'Very Long Modpack Name Pa ck';
    const out = defaultCloneName(name, ' (copy)');
    expect(out).not.toContain('  ');
    expect(out.length).toBeLessThanOrEqual(INSTANCE_NAME_MAX);
  });

  it('never exceeds the limit even with an oversized suffix', () => {
    const out = defaultCloneName('Name', 'x'.repeat(INSTANCE_NAME_MAX + 5));
    expect(out.length).toBeLessThanOrEqual(INSTANCE_NAME_MAX);
  });
});
