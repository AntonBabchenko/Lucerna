// The build identity in words: the line under the version, and the block
// "Copy version info" puts on the clipboard for a bug report.
import { get } from 'svelte/store';
import { describe, expect, it } from 'vitest';
import { t } from '$lib/i18n';
import type { BuildInfo } from '$lib/ipc/bindings';
import { buildLine, versionInfoBlock } from '$lib/settings/version-info';

const base: BuildInfo = {
  version: '0.25.0',
  build: { kind: 'tagged', tag: 'v0.25.0-rc.2' },
  commit: 'a1b2c3d',
  fork: null,
  os: 'windows',
  arch: 'x86_64',
};
const tr = () => get(t);

describe('the build line', () => {
  it('names the tag, the commit and the platform of an official build', () => {
    expect(buildLine(base, tr())).toBe('v0.25.0-rc.2 · a1b2c3d · windows x86_64');
  });

  it('leaves out a commit the build did not record', () => {
    expect(buildLine({ ...base, commit: null }, tr())).toBe('v0.25.0-rc.2 · windows x86_64');
  });

  it('says a local or development build is one', () => {
    expect(buildLine({ ...base, build: { kind: 'local' }, commit: null }, tr())).toBe(
      'Local build · windows x86_64',
    );
    expect(buildLine({ ...base, build: { kind: 'development' }, commit: null }, tr())).toBe(
      'Development build · windows x86_64',
    );
  });

  it('shows an unrecognised tag as it is, marked, rather than calling the build local', () => {
    expect(
      buildLine({ ...base, build: { kind: 'unrecognised', raw: 'main' }, commit: null }, tr()),
    ).toBe('main (unrecognised) · windows x86_64');
  });

  it('says when the build came from another repository', () => {
    expect(buildLine({ ...base, fork: 'someone/Lucerna' }, tr())).toBe(
      'v0.25.0-rc.2 · a1b2c3d · windows x86_64 · fork: someone/Lucerna',
    );
  });
});

describe('the copied block', () => {
  it('is three paste-ready English lines for an official build', () => {
    expect(versionInfoBlock(base, '0.25.0')).toBe(
      'Lucerna 0.25.0\nBuild: v0.25.0-rc.2 (a1b2c3d)\nOS: windows x86_64',
    );
  });

  it('names local, development and unrecognised builds', () => {
    expect(
      versionInfoBlock({ ...base, build: { kind: 'local' }, commit: null }, '0.25.0'),
    ).toContain('\nBuild: local\n');
    expect(
      versionInfoBlock({ ...base, build: { kind: 'development' }, commit: null }, '0.25.0'),
    ).toContain('\nBuild: development\n');
    expect(
      versionInfoBlock(
        { ...base, build: { kind: 'unrecognised', raw: 'main' }, commit: null },
        '0.25.0',
      ),
    ).toContain('\nBuild: main (unrecognised)\n');
  });

  it('adds the fork on its own line', () => {
    expect(versionInfoBlock({ ...base, fork: 'someone/Lucerna' }, '0.25.0')).toBe(
      'Lucerna 0.25.0\nBuild: v0.25.0-rc.2 (a1b2c3d)\nFork: someone/Lucerna\nOS: windows x86_64',
    );
  });

  it('says unknown, never blank, when the build info could not be read', () => {
    expect(versionInfoBlock(null, '0.24.0')).toBe('Lucerna 0.24.0\nBuild: unknown\nOS: unknown');
  });
});
