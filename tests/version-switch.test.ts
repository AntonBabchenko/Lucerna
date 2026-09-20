import { describe, expect, it } from 'vitest';
import type { ModVersion } from '$lib/ipc/bindings';
import { isInstalledBuild, switchTarget } from '$lib/mods/version-switch';

function build(versionId: string, sha1: string | null): ModVersion {
  return {
    source: 'modrinth',
    project_id: 'p',
    version_id: versionId,
    name: `Build ${versionId}`,
    version_number: versionId,
    mc_versions: ['1.21.1'],
    loaders: ['neoforge'],
    primary_file: {
      filename: `${versionId}.jar`,
      url: '',
      sha1,
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
  };
}

describe('isInstalledBuild', () => {
  it('recognises the installed build by its version id', () => {
    // (pin)
    expect(isInstalledBuild(build('v1', 'aaa'), 'v1', null)).toBe(true);
    expect(isInstalledBuild(build('v2', 'bbb'), 'v1', null)).toBe(false);
  });

  it('recognises it by its bytes when the registry has no version id', () => {
    // `enrich` leaves `version_id` null for exactly the mods whose tags
    // disagree with the instance — the population this feature is for.
    expect(isInstalledBuild(build('v1', 'ABCDEF'), null, 'abcdef')).toBe(true);
  });

  it('never matches on an absent or empty digest', () => {
    // (pin) CurseForge may publish no sha1; two unknowns are not «the same».
    expect(isInstalledBuild(build('v1', null), null, null)).toBe(false);
    expect(isInstalledBuild(build('v1', ''), null, '')).toBe(false);
  });
});

describe('switchTarget', () => {
  it('is a fresh install when nothing is installed', () => {
    // (pin)
    expect(switchTarget(null, build('v2', 'bbb'))).toBeNull();
  });

  it('replaces the installed jar when another build is picked', () => {
    // (pin)
    expect(switchTarget({ sha1: 'aaa', version_id: 'v1' }, build('v2', 'bbb'))).toBe('aaa');
    expect(switchTarget({ sha1: 'aaa', version_id: null }, build('v2', 'bbb'))).toBe('aaa');
  });

  it('is not a switch when the picked build IS the installed one, id or no id', () => {
    expect(switchTarget({ sha1: 'aaa', version_id: 'v1' }, build('v1', 'aaa'))).toBeNull();
    // Without a version id every click used to look like a switch, and ran an
    // uninstall-and-reinstall of the very build that was already there.
    expect(switchTarget({ sha1: 'aaa', version_id: null }, build('v1', 'AAA'))).toBeNull();
  });
});
