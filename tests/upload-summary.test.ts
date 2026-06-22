import { describe, expect, it } from 'vitest';
import type { Translate } from '$lib/i18n';
import { formatLastUpload, type PreflightView, preflightLevel } from '$lib/servers/upload-summary';

// A minimal fake $t that echoes the key + interpolations so assertions are
// deterministic and locale-independent.
const t: Translate = ((key: string, vars?: Record<string, unknown>) =>
  vars ? `${key}:${JSON.stringify(vars)}` : key) as unknown as Translate;

describe('formatLastUpload', () => {
  it('returns null when there is no last upload', () => {
    expect(formatLastUpload(t, null)).toBeNull();
    expect(formatLastUpload(t, undefined)).toBeNull();
  });

  it('formats the when + target into the i18n line', () => {
    const out = formatLastUpload(t, { unix_ms: 1_700_000_000_000, target: 'h:22/srv' });
    expect(out).not.toBeNull();
    expect(out as string).toContain('servers.hosting.lastUpload');
    expect(out as string).toContain('h:22/srv');
  });
});

describe('preflightLevel', () => {
  const base: PreflightView = { total_bytes: 100, free_bytes: 50, exceeds_free: true };

  it('is "over" when the upload exceeds known free space', () => {
    expect(preflightLevel(base)).toBe('over');
  });

  it('is "ok" when it fits', () => {
    expect(preflightLevel({ total_bytes: 30, free_bytes: 100, exceeds_free: false })).toBe('ok');
  });

  it('is "unknown" when free space is not reported', () => {
    expect(preflightLevel({ total_bytes: 30, free_bytes: null, exceeds_free: false })).toBe(
      'unknown',
    );
  });
});
