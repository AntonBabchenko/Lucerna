import { beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import { formatError, javaUnusableReasonText } from '$lib/ipc/format-error';

// A Java the user chose for an instance and the launcher refused: the message
// names the file and the reason, in the user's language (spec 2026-10-11
// custom-java §10).
describe('a chosen Java that cannot be used', () => {
  beforeAll(() => locale.set('en'));

  it('names the path and the reason', () => {
    const s = formatError({
      kind: 'java_unusable',
      path: 'C:\\jdk8\\bin\\javaw.exe',
      reason: { kind: 'too_old', found: 8, required: 21 },
    });
    expect(s).toContain('C:\\jdk8\\bin\\javaw.exe');
    expect(s).toContain('Java 8');
    expect(s).toContain('Java 21');
  });

  it('says a missing file is missing, not that it could not be checked', () => {
    expect(javaUnusableReasonText({ kind: 'not_found' })).toMatch(/isn't there/);
    expect(javaUnusableReasonText({ kind: 'probe_failed', details: 'timed out' })).toContain(
      'timed out',
    );
  });

  it('cuts a long probe answer to its first line', () => {
    const details = `${'x'.repeat(300)}\nsecond line`;
    const s = javaUnusableReasonText({ kind: 'probe_failed', details });
    expect(s).not.toContain('second line');
    expect(s).not.toContain('x'.repeat(200));
  });
});
