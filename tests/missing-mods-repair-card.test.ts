import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const modsInstallWithDeps = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsInstallWithDeps: (...a: unknown[]) => modsInstallWithDeps(...a),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

import type { RepairPlan, VersionRef } from '$lib/ipc/bindings';
import MissingModsRepairCard from '$lib/logs/MissingModsRepairCard.svelte';

function makeVersionRef(projectId: string): VersionRef {
  return { source: 'modrinth', project_id: projectId, version_id: `v-${projectId}` };
}

const planWithExactAndUnresolved: Extract<RepairPlan, { kind: 'install_missing_mods' }> = {
  kind: 'install_missing_mods',
  mods: [
    {
      cited: { id: 'create', version: '0.5.1', kind: 'missing' },
      tier: {
        tier: 'exact',
        candidate: {
          target: makeVersionRef('p1'),
          display: {
            source: 'modrinth',
            project_id: 'p1',
            slug: 'create',
            name: 'Create',
            summary: 'A Minecraft mod',
            icon_url: null,
            downloads: 0,
            author: 'simibubi',
            updated_at: null,
          },
          version_label: '0.5.1',
          dependencies: [],
        },
      },
    },
    {
      cited: { id: 'ghostmod', version: null, kind: 'missing' },
      tier: { tier: 'unresolved' },
    },
  ],
};

const planWithFuzzyOnly: Extract<RepairPlan, { kind: 'install_missing_mods' }> = {
  kind: 'install_missing_mods',
  mods: [
    {
      cited: { id: 'somemod', version: '1.0.0', kind: 'missing' },
      tier: {
        tier: 'fuzzy',
        candidates: [
          {
            target: makeVersionRef('cand1'),
            display: {
              source: 'modrinth',
              project_id: 'cand1',
              slug: 'some-mod-a',
              name: 'Some Mod A',
              summary: '',
              icon_url: null,
              downloads: 0,
              author: 'author1',
              updated_at: null,
            },
            version_label: '1.0.0',
            dependencies: [],
          },
          {
            target: makeVersionRef('cand2'),
            display: {
              source: 'modrinth',
              project_id: 'cand2',
              slug: 'some-mod-b',
              name: 'Some Mod B',
              summary: '',
              icon_url: null,
              downloads: 0,
              author: 'author2',
              updated_at: null,
            },
            version_label: '1.0.1',
            dependencies: [],
          },
        ],
      },
    },
  ],
};

const planWithExactWithDeps: Extract<RepairPlan, { kind: 'install_missing_mods' }> = {
  kind: 'install_missing_mods',
  mods: [
    {
      cited: { id: 'fd-ntp-compat', version: null, kind: 'missing' },
      tier: {
        tier: 'exact',
        candidate: {
          target: makeVersionRef('patch123'),
          display: {
            source: 'modrinth',
            project_id: 'patch123',
            slug: 'fd-ntp-compat',
            name: 'FD x NTP Cooking Pot',
            summary: 'Compat patch',
            icon_url: null,
            downloads: 1000,
            author: 'patchauthor',
            updated_at: null,
          },
          version_label: '1.0.0',
          dependencies: ['No Tree Punching'],
        },
      },
    },
  ],
};

describe('MissingModsRepairCard', () => {
  it('pre-checks exact matches and enables the install button; lists unresolved mods', () => {
    render(MissingModsRepairCard, {
      props: {
        plan: planWithExactAndUnresolved,
        instanceId: 'i1',
        onClose: vi.fn(),
      },
    });

    // Exact match checkbox should be present and checked
    const exactCheckbox = screen.getByTestId('missing-exact-p1');
    expect(exactCheckbox).toBeTruthy();
    expect((exactCheckbox as HTMLInputElement).checked).toBe(true);

    // Unresolved mod id should appear in the DOM
    expect(screen.getByTestId('missing-unresolved-ghostmod')).toBeTruthy();
    expect(screen.getByText('ghostmod')).toBeTruthy();

    // Install button is enabled (1 exact mod selected)
    const installBtn = screen.getByTestId('missing-install');
    expect((installBtn as HTMLButtonElement).disabled).toBe(false);
  });

  it('disables the install button when the only resolvable mod is fuzzy and nothing is selected', () => {
    render(MissingModsRepairCard, {
      props: {
        plan: planWithFuzzyOnly,
        instanceId: 'i1',
        onClose: vi.fn(),
      },
    });

    // No radio selected by default → install button disabled
    const installBtn = screen.getByTestId('missing-install');
    expect((installBtn as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows dependsOn line when candidate has dependencies', () => {
    render(MissingModsRepairCard, {
      props: {
        plan: planWithExactWithDeps,
        instanceId: 'i1',
        onClose: vi.fn(),
      },
    });

    // The dependsOn line should render with the dep name
    expect(screen.getByText(/No Tree Punching/)).toBeTruthy();
  });
});
