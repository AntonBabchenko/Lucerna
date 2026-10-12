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
      origin: { kind: 'instance' },
      reason: { kind: 'too_old', found: 8, required: 21 },
    });
    expect(s).toContain('C:\\jdk8\\bin\\javaw.exe');
    expect(s).toContain('Java 8');
    expect(s).toContain('Java 21');
  });

  // The fix lives where the Java was chosen: a refused Settings default must
  // not send the user to the instance's field, which holds no path.
  it('names the place the Java was chosen', () => {
    const base = {
      kind: 'java_unusable',
      path: 'C:\\jdk17\\bin\\javaw.exe',
      reason: { kind: 'not_found' },
    } as const;
    expect(formatError({ ...base, origin: { kind: 'instance' } })).toMatch(/this instance/);
    const fromSettings = formatError({ ...base, origin: { kind: 'default', major: 17 } });
    expect(fromSettings).toMatch(/Settings/);
    expect(fromSettings).toContain('Java 17');
    expect(formatError({ ...base, origin: { kind: 'server' } })).toMatch(/this server/);
  });

  it('says a missing file is missing, not that it could not be checked', () => {
    expect(javaUnusableReasonText({ kind: 'not_found' })).toMatch(/isn't there/);
    expect(javaUnusableReasonText({ kind: 'probe_failed', details: 'timed out' })).toContain(
      'timed out',
    );
  });

  it("says which step could not get Lucerna's runtime, and why", () => {
    const forGame = formatError({
      kind: 'jre_unavailable',
      component: 'java-runtime-delta',
      major: 21,
      stage: 'game_runtime',
      reason: { kind: 'unsupported_platform', os: 'linux', arch: 'riscv64' },
    });
    expect(forGame).toContain('Java 21');
    expect(forGame).toContain('riscv64');
    const forInstaller = formatError({
      kind: 'jre_unavailable',
      component: 'java-runtime-delta',
      major: 21,
      stage: 'installer_tools',
      reason: { kind: 'download', details: `refused\n${'y'.repeat(300)}` },
    });
    expect(forInstaller).toMatch(/Forge|NeoForge/);
    expect(forInstaller).toContain('refused');
    expect(forInstaller).not.toContain('y'.repeat(200));
  });

  it('cuts a long probe answer to its first line', () => {
    const details = `${'x'.repeat(300)}\nsecond line`;
    const s = javaUnusableReasonText({ kind: 'probe_failed', details });
    expect(s).not.toContain('second line');
    expect(s).not.toContain('x'.repeat(200));
  });
});
