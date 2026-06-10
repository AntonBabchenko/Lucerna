import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ModVersion } from '$lib/ipc/bindings';
import DependencyDialog from '$lib/mods/DependencyDialog.svelte';

function v(projectId: string, vid: string): ModVersion {
  return {
    source: 'modrinth',
    project_id: projectId,
    version_id: vid,
    name: `${projectId}-release`,
    version_number: '1.0',
    mc_versions: [],
    loaders: [],
    primary_file: {
      filename: '',
      url: '',
      sha1: null,
      size: 0,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
  };
}

function dep(projectName: string, version: ModVersion) {
  return { version, projectName, projectSource: 'modrinth' as const, requires: [] };
}

describe('DependencyDialog', () => {
  it('shows install count = 1 + required + checked-optional', async () => {
    const onConfirm = vi.fn();
    const oVersion = v('O', 'vo');
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'Primary Mod',
        required: [dep('Required Mod', v('R', 'vr'))],
        optional: [dep('Optional Mod', oVersion)],
        incompatible: [],
        unresolvable: [],
        onCancel: () => {},
        onConfirm,
      },
    });
    expect(screen.getByRole('button', { name: /Install \(2 mods\)/ })).toBeTruthy();
    await fireEvent.click(screen.getByRole('checkbox'));
    expect(screen.getByRole('button', { name: /Install \(3 mods\)/ })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /Install \(3 mods\)/ }));
    expect(onConfirm).toHaveBeenCalledWith([oVersion]);
  });

  it('renders the project name (not the version title) for each dep', () => {
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'Jade',
        required: [dep('Jade Addons', v('JA', 'vja'))],
        optional: [],
        incompatible: [],
        unresolvable: [],
        onCancel: () => {},
        onConfirm: () => {},
      },
    });
    expect(screen.getByText(/Jade Addons/)).toBeTruthy();
    expect(screen.queryByText(/JA-release/)).toBeNull();
  });

  it('shows incompatible warning row', () => {
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'X',
        required: [],
        optional: [],
        incompatible: ['rei'],
        unresolvable: [],
        onCancel: () => {},
        onConfirm: () => {},
      },
    });
    expect(screen.getByText(/Incompatible with: rei/)).toBeTruthy();
  });

  it('surfaces loader requirements as an info row without installing them', () => {
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'Jade',
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [],
        loaderRequirements: ['NeoForge'],
        onCancel: () => {},
        onConfirm: () => {},
      },
    });
    expect(screen.getByText(/This mod targets NeoForge/)).toBeTruthy();
    // Count is 1 (primary only) — loader requirements do NOT bump it.
    expect(screen.getByRole('button', { name: /Install \(1 mod\)/ })).toBeTruthy();
  });

  it('renders a noMc unresolvable row with the per-loader list', () => {
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'X',
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [{ kind: 'noMc', name: 'Jade', mcVersion: '1.20.4', list: 'Forge 1.19.2' }],
        onCancel: () => {},
        onConfirm: () => {},
      },
    });
    expect(
      screen.getByText(/Jade — no version for Minecraft 1\.20\.4\. Latest: Forge 1\.19\.2/),
    ).toBeTruthy();
  });

  it('renders a wrongLoader unresolvable row naming the instance loader', () => {
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'X',
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [{ kind: 'wrongLoader', name: 'Balm', loader: 'Forge', list: 'Fabric 1.21' }],
        onCancel: () => {},
        onConfirm: () => {},
      },
    });
    expect(screen.getByText(/Balm — no Forge version\. Available: Fabric 1\.21/)).toBeTruthy();
  });

  it('renders a noVersions unresolvable row with just the name', () => {
    render(DependencyDialog, {
      props: {
        primary: v('X', 'vx'),
        primaryProjectName: 'X',
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [{ kind: 'noVersions', name: 'Ghost' }],
        onCancel: () => {},
        onConfirm: () => {},
      },
    });
    expect(screen.getByText(/Ghost — no compatible version found/)).toBeTruthy();
  });
});
