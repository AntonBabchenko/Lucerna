import { describe, expect, it } from 'vitest';
import type { PreflightReport } from '$lib/ipc/bindings';
import { hasBlocking, toOverlayKeys } from '$lib/mods/preflight.svelte';

const report: PreflightReport = {
  violations: [
    {
      kind: 'version_out_of_range',
      dependent_name: 'Sophisticated Backpacks',
      dependent_sha1: 'aa',
      dep_id: 'sophisticatedcore',
      dep_display_name: 'Sophisticated Core',
      needed: '[1.3.51,)',
      installed_version: '1.3.50.2005',
      provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
    },
  ],
};

describe('preflight overlay mapping', () => {
  it('exposes modrinth:core-id as an out-of-range overlay key', () => {
    expect(toOverlayKeys(report)).toContain('modrinth:core-id');
  });
  it('treats a non-empty report as blocking', () => {
    expect(hasBlocking(report)).toBe(true);
    expect(hasBlocking({ violations: [] })).toBe(false);
  });
});

describe('toOverlayKeys edge cases', () => {
  it('ignores missing_required violations (no provider_project needed)', () => {
    const missingReport: PreflightReport = {
      violations: [
        {
          kind: 'missing_required',
          dependent_name: 'Backpacks',
          dependent_sha1: 'bb',
          dep_id: 'missingmod',
          dep_display_name: null,
          needed: '',
          installed_version: null,
          provider_project: null,
        },
      ],
    };
    expect(toOverlayKeys(missingReport).size).toBe(0);
  });

  it('ignores version_out_of_range violations with no provider_project', () => {
    const noProviderReport: PreflightReport = {
      violations: [
        {
          kind: 'version_out_of_range',
          dependent_name: 'SomeMod',
          dependent_sha1: 'cc',
          dep_id: 'unknowndep',
          dep_display_name: null,
          needed: '[1.0,)',
          installed_version: '0.9',
          provider_project: null,
        },
      ],
    };
    expect(toOverlayKeys(noProviderReport).size).toBe(0);
  });

  it('maps curseforge provider_project to curseforge:mod_id', () => {
    const cfReport: PreflightReport = {
      violations: [
        {
          kind: 'version_out_of_range',
          dependent_name: 'SomeMod',
          dependent_sha1: 'dd',
          dep_id: 'cfmod',
          dep_display_name: null,
          needed: '[2.0,)',
          installed_version: '1.9',
          provider_project: { source: 'curseforge', mod_id: 12345, file_id: null },
        },
      ],
    };
    expect(toOverlayKeys(cfReport)).toContain('curseforge:12345');
  });

  it('returns an empty set for an empty report', () => {
    expect(toOverlayKeys({ violations: [] }).size).toBe(0);
  });
});
