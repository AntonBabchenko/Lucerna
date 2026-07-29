import { describe, expect, it } from 'vitest';
import type { ModpackVersionEntry } from '$lib/ipc/bindings';
import {
  assessSwitchRisks,
  packChangelogBase,
  sortVersionsNewestFirst,
  switchDirection,
} from '$lib/modpacks/switch-risks';

function ver(id: string, date: string): ModpackVersionEntry {
  return {
    id,
    name: id,
    version_number: id,
    game_versions: ['1.20.1'],
    loaders: ['fabric'],
    date_published: date,
  };
}

// Deliberately unsorted so the helpers cannot rely on input order.
const LIST: ModpackVersionEntry[] = [
  ver('v1', '2026-01-01T00:00:00Z'),
  ver('v3', '2026-03-01T00:00:00Z'),
  ver('v2', '2026-02-01T00:00:00Z'),
];

describe('sortVersionsNewestFirst', () => {
  it('orders by publish date, newest first', () => {
    expect(sortVersionsNewestFirst(LIST).map((v) => v.id)).toEqual(['v3', 'v2', 'v1']);
  });

  it('does not mutate the input array', () => {
    const input = [...LIST];
    sortVersionsNewestFirst(input);
    expect(input.map((v) => v.id)).toEqual(['v1', 'v3', 'v2']);
  });
});

describe('switchDirection', () => {
  it('is an upgrade when the target was published later than the installed version', () => {
    expect(switchDirection(LIST, 'v1', 'v3')).toBe('upgrade');
  });

  it('is a downgrade when the target was published earlier', () => {
    expect(switchDirection(LIST, 'v3', 'v1')).toBe('downgrade');
  });

  it('is a reinstall when the target is the installed version', () => {
    expect(switchDirection(LIST, 'v2', 'v2')).toBe('reinstall');
  });

  it('is unknown when the installed version is not in the list', () => {
    // Delisted version, or a drag-drop import with no provenance.
    expect(switchDirection(LIST, 'gone', 'v2')).toBe('unknown');
  });

  it('is unknown when the installed version id is null', () => {
    expect(switchDirection(LIST, null, 'v2')).toBe('unknown');
  });
});

describe('assessSwitchRisks', () => {
  const noRiskInput = {
    direction: 'upgrade' as const,
    versionBump: null,
    userAdded: 0,
    manual: 0,
    hasBundledFiles: false,
  };

  it('reports no risks for a clean same-Minecraft upgrade of an uncustomized pack', () => {
    expect(assessSwitchRisks(noRiskInput)).toEqual([]);
  });

  it('reports a downgrade risk when going backwards', () => {
    expect(assessSwitchRisks({ ...noRiskInput, direction: 'downgrade' })).toEqual([
      { kind: 'downgrade' },
    ]);
  });

  it('treats an unknown installed version as a downgrade', () => {
    // We cannot prove it is not one, so we must not imply it is safe.
    expect(assessSwitchRisks({ ...noRiskInput, direction: 'unknown' })).toEqual([
      { kind: 'downgrade' },
    ]);
  });

  it('reports no downgrade risk for a reinstall', () => {
    expect(assessSwitchRisks({ ...noRiskInput, direction: 'reinstall' })).toEqual([]);
  });

  it('reports a Minecraft change without claiming which version is older', () => {
    const risks = assessSwitchRisks({
      ...noRiskInput,
      versionBump: {
        old_game_version: '1.20.1',
        new_game_version: '1.19.2',
        old_loader_version: '0.15.0',
        new_loader_version: '0.15.0',
      },
    });
    expect(risks).toEqual([{ kind: 'mc-change', from: '1.20.1', to: '1.19.2' }]);
  });

  it('does not report a Minecraft change when only the loader version moved', () => {
    const risks = assessSwitchRisks({
      ...noRiskInput,
      versionBump: {
        old_game_version: '1.20.1',
        new_game_version: '1.20.1',
        old_loader_version: '0.15.0',
        new_loader_version: '0.16.0',
      },
    });
    expect(risks).toEqual([{ kind: 'loader-change', from: '0.15.0', to: '0.16.0' }]);
  });

  it('renders a null loader version as a dash rather than "null"', () => {
    const risks = assessSwitchRisks({
      ...noRiskInput,
      versionBump: {
        old_game_version: '1.20.1',
        new_game_version: '1.20.1',
        old_loader_version: null,
        new_loader_version: '0.16.0',
      },
    });
    expect(risks).toEqual([{ kind: 'loader-change', from: '—', to: '0.16.0' }]);
  });

  it('counts user-added and manually-dropped mods separately', () => {
    expect(assessSwitchRisks({ ...noRiskInput, userAdded: 3, manual: 2 })).toEqual([
      { kind: 'customizations', userAdded: 3, manual: 2 },
    ]);
  });

  it('reports no customizations risk when the user added nothing', () => {
    expect(assessSwitchRisks({ ...noRiskInput, userAdded: 0, manual: 0 })).toEqual([]);
  });

  it('reports the bundled-overrides risk when the pack bundles files', () => {
    expect(assessSwitchRisks({ ...noRiskInput, hasBundledFiles: true })).toEqual([
      { kind: 'bundled-overrides' },
    ]);
  });

  it('orders risks most-consequential first', () => {
    const risks = assessSwitchRisks({
      direction: 'downgrade',
      versionBump: {
        old_game_version: '1.20.1',
        new_game_version: '1.19.2',
        old_loader_version: '0.15.0',
        new_loader_version: '0.14.0',
      },
      userAdded: 1,
      manual: 0,
      hasBundledFiles: true,
    });
    expect(risks.map((r) => r.kind)).toEqual([
      'mc-change',
      'downgrade',
      'loader-change',
      'customizations',
      'bundled-overrides',
    ]);
  });
});

describe('packChangelogBase', () => {
  it('uses the instance version id, not the pack origin version number', () => {
    expect(packChangelogBase({ mrpack_version_id: 'vid-abc' })).toBe('vid-abc');
  });

  it('is null when the instance has no recorded version id', () => {
    expect(packChangelogBase({ mrpack_version_id: null })).toBeNull();
  });
});
