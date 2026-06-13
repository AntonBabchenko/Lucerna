import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { InstalledMod, ModSummary } from '$lib/ipc/bindings';
import ModCard from '$lib/mods/ModCard.svelte';

const summary: ModSummary = {
  source: 'modrinth',
  project_id: 'p1',
  slug: 'mod',
  name: 'Test Mod',
  summary: 'Does things',
  icon_url: null,
  downloads: 1234,
  author: 'someone',
  updated_at: null,
};

function inst(enabled: boolean): InstalledMod {
  return {
    filename: 'm.jar',
    sha1: 'abc',
    source: 'modrinth',
    project_id: 'p1',
    version_id: 'v1',
    name: 'Test Mod',
    version_number: '1.0',
    installed_at: '2026-05-19T00:00:00Z',
    enabled,
  };
}

const noopProps = {
  onInstall: () => {},
  onOpenDetail: () => {},
  onToggle: () => {},
  onUninstall: () => {},
};

describe('ModCard', () => {
  it('shows Install button when the mod is not installed', () => {
    render(ModCard, { props: { summary, installed: null, ...noopProps } });
    expect(screen.getByRole('button', { name: /^install$/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /uninstall/i })).toBeNull();
  });

  it('shows Disable + Uninstall + Installed pill when the mod is installed and enabled', () => {
    render(ModCard, { props: { summary, installed: inst(true), ...noopProps } });
    expect(screen.getByText(/^Installed/)).toBeTruthy();
    expect(screen.getByRole('button', { name: /^disable$/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /uninstall/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /^install$/i })).toBeNull();
  });

  it('shows Enable + Disabled pill when the mod is installed but disabled', () => {
    render(ModCard, { props: { summary, installed: inst(false), ...noopProps } });
    expect(screen.getByText(/^Disabled/)).toBeTruthy();
    expect(screen.getByRole('button', { name: /^enable$/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /uninstall/i })).toBeTruthy();
  });

  it('shows the version number on the Installed pill when known', () => {
    render(ModCard, { props: { summary, installed: inst(true), ...noopProps } });
    expect(screen.getByText(/Installed · v1\.0/)).toBeTruthy();
  });

  it('shows the other-platform label when installed.source ≠ summary.source', () => {
    // Card is rendered for the CurseForge entry of Cloth Config API,
    // but the mod is already on disk via Modrinth (same display name).
    // installedFor in the parent matches by name, so this card receives
    // an InstalledMod whose source is "modrinth" while summary.source
    // is "curseforge". The version number from the other platform
    // wouldn't make sense next to CF's version listings, so the pill
    // hides v* and shows "(Modrinth)" instead.
    const cfSummary: ModSummary = { ...summary, source: 'curseforge', project_id: '12345' };
    render(ModCard, { props: { summary: cfSummary, installed: inst(true), ...noopProps } });
    expect(screen.getByText(/Installed \(Modrinth\)/)).toBeTruthy();
    expect(screen.queryByText(/v1\.0/)).toBeNull();
  });

  it('reverses the other-platform label when installed on CF and viewed on Modrinth', () => {
    const cfInstalled: InstalledMod = {
      ...inst(true),
      source: 'curseforge',
      project_id: '12345',
    };
    render(ModCard, { props: { summary, installed: cfInstalled, ...noopProps } });
    expect(screen.getByText(/Installed \(CurseForge\)/)).toBeTruthy();
  });

  it('list row shows bg-highlight when highlighted={true}', () => {
    const { container } = render(ModCard, {
      props: { summary, installed: inst(true), layout: 'list', highlighted: true, ...noopProps },
    });
    const row = container.querySelector('[data-testid="card-list-row"]');
    expect(row?.className).toContain('bg-highlight');
  });

  it('list row keeps bg-surface (no bg-highlight) when highlighted is false', () => {
    const { container } = render(ModCard, {
      props: { summary, installed: inst(true), layout: 'list', ...noopProps },
    });
    const row = container.querySelector('[data-testid="card-list-row"]');
    expect(row?.className).toContain('bg-surface');
    expect(row?.className).not.toContain('bg-highlight');
  });

  it('compact list shows icon action buttons (power + uninstall) for an enabled mod', () => {
    render(ModCard, {
      props: { summary, installed: inst(true), layout: 'list', density: 'compact', ...noopProps },
    });
    // Enabled → power toggle labelled "Disable", plus an "Uninstall" icon button.
    expect(screen.getByRole('button', { name: /disable/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /uninstall/i })).toBeTruthy();
  });

  it('compact list omits the toggle for resource packs/shaders (canToggle=false)', () => {
    render(ModCard, {
      props: {
        summary,
        installed: inst(true),
        layout: 'list',
        density: 'compact',
        canToggle: false,
        ...noopProps,
      },
    });
    expect(screen.queryByRole('button', { name: /disable/i })).toBeNull();
    expect(screen.getByRole('button', { name: /uninstall/i })).toBeTruthy();
  });

  it('an enabled mod keeps a quiet (transparent) accent strip — no wall of green', () => {
    const { container } = render(ModCard, {
      props: { summary, installed: inst(true), layout: 'list', ...noopProps },
    });
    expect(container.querySelector('[data-card-accent]')?.className).toContain('bg-transparent');
  });

  it('an incompatible installed mod paints a danger accent strip', () => {
    const { container } = render(ModCard, {
      props: {
        summary,
        installed: inst(true),
        layout: 'list',
        attention: 'incompatible',
        ...noopProps,
      },
    });
    expect(container.querySelector('[data-card-accent]')?.className).toContain('bg-danger');
  });
});
