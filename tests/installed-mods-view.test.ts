import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// Capture each listener callback at module-eval time via vi.hoisted so
// the vi.mock factory (which is itself hoisted) can write through to
// this registry. Tests can then poke the listener to simulate a live
// event from the backend. The real bindings expose a single `modToggle`
// event for both enable and disable transitions — not two separate ones.
const listeners = vi.hoisted(() => ({
  modInstalled: null as null | (() => void),
  modUninstalled: null as null | (() => void),
  modToggle: null as null | (() => void),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // Both rows return the typedError ok-shape. The first is a normal
    // Modrinth install (source set, version + project ids present); the
    // second is a "manual" mod — a JAR the user dropped into the
    // instance's mods folder by hand (source: null).
    modsListInstalled: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        {
          filename: 'jei.jar',
          sha1: 'abc',
          source: 'modrinth',
          project_id: 'p',
          version_id: 'v',
          name: 'Just Enough Items',
          version_number: '15.0',
          installed_at: '2026-05-18T00:00:00Z',
          enabled: true,
        },
        {
          filename: 'mystery.jar',
          sha1: 'def',
          source: null,
          project_id: null,
          version_id: null,
          name: 'mystery.jar',
          version_number: null,
          installed_at: '2026-05-18T00:00:00Z',
          enabled: false,
        },
      ],
    }),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    // The view now fetches ModProject for each platform-installed mod
    // so it can render the project's display name via the shared
    // ModCard component.
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'p',
          slug: 'jei',
          name: 'Just Enough Items',
          summary: 'View items and recipes',
          icon_url: null,
          downloads: 1234,
          author: 'mezz',
          updated_at: null,
        },
        description: '',
        website_url: null,
      },
    }),
  },
  events: {
    modInstalled: {
      listen: (cb: () => void) => {
        listeners.modInstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modUninstalled: {
      listen: (cb: () => void) => {
        listeners.modUninstalled = cb;
        return Promise.resolve(() => {});
      },
    },
    modToggle: {
      listen: (cb: () => void) => {
        listeners.modToggle = cb;
        return Promise.resolve(() => {});
      },
    },
  },
}));

import InstalledModsView from '$lib/mods/InstalledModsView.svelte';

describe('InstalledModsView', () => {
  it('renders rows with Disable button when enabled and Enable when disabled', async () => {
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // Yield once so the mount-time refresh() promise resolves before we
    // assert on the rendered rows.
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText('Just Enough Items')).toBeTruthy();
    // Enabled row → Disable button; Disabled row → Enable button.
    expect(screen.getByRole('button', { name: 'Disable' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Enable' })).toBeTruthy();
  });

  it('calls modsUninstall when Uninstall clicked', async () => {
    const mod = await import('$lib/ipc/bindings');
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    const buttons = screen.getAllByRole('button', { name: 'Uninstall' });
    const firstButton = buttons[0];
    if (!firstButton) throw new Error('expected at least one Uninstall button');
    await fireEvent.click(firstButton);
    expect(mod.commands.modsUninstall).toHaveBeenCalledWith('i', 'abc');
  });

  it('shows empty state when no instance is selected', () => {
    render(InstalledModsView, {
      props: { instanceId: null, mcVersion: null, loader: null },
    });
    expect(screen.getByText(/Pick an instance first/)).toBeTruthy();
  });

  it('checks for updates and renders an update badge', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsCheckUpdates as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          sha1: 'abc',
          name: 'Just Enough Items',
          source: 'modrinth',
          project_id: 'p',
          current_version_id: 'v',
          current_version_number: '15.0',
          state: {
            kind: 'update_available',
            target: {
              source: 'modrinth',
              project_id: 'p',
              version_id: 'v2',
              name: 'Just Enough Items',
              version_number: '16.0',
              mc_versions: ['1.20.1'],
              loaders: ['fabric'],
              primary_file: {
                filename: 'jei-16.jar',
                url: 'https://example/jei-16.jar',
                sha1: 'ffff',
                size: 1,
                distribution_allowed: true,
              },
              deps: [],
              published_at: null,
            },
          },
        },
      ],
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    await fireEvent.click(screen.getByRole('button', { name: /Check for updates/ }));
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.modsCheckUpdates).toHaveBeenCalledWith('i');
    expect(screen.getByText('v15.0 → v16.0')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Update' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Update all (1)' })).toBeTruthy();
  });

  it('marks a modpack-origin mod with a pack chip', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Cool Pack', mod_shas: ['abc'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/📦 Cool Pack/)).toBeTruthy();
  });

  it('labels an unresolved pack-origin mod "from modpack" with a chip', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          filename: 'bundled.jar',
          sha1: 'pack1',
          source: null,
          project_id: null,
          version_id: null,
          name: 'bundled.jar',
          version_number: null,
          installed_at: '2026-05-18T00:00:00Z',
          enabled: true,
          // already attempted — suppresses the backfill for this test
          enrich_attempted: true,
        },
      ],
    });
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Parasites Reloaded', mod_shas: ['pack1'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/from modpack/)).toBeTruthy();
    expect(screen.getByText(/📦 Parasites Reloaded/)).toBeTruthy();
  });

  it('keeps "manual mod" for a hand-dropped jar not in the pack', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: [
        {
          filename: 'handdrop.jar',
          sha1: 'hand1',
          source: null,
          project_id: null,
          version_id: null,
          name: 'handdrop.jar',
          version_number: null,
          installed_at: '2026-05-18T00:00:00Z',
          enabled: true,
          enrich_attempted: true,
        },
      ],
    });
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Some Pack', mod_shas: ['other-sha'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText(/manual mod/)).toBeTruthy();
  });

  it('runs an enrichment backfill when a pack mod is unenriched', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>).mockClear();
    const unresolved = {
      filename: 'bundled.jar',
      sha1: 'pack1',
      source: null,
      project_id: null,
      version_id: null,
      name: 'bundled.jar',
      version_number: null,
      installed_at: '2026-05-18T00:00:00Z',
      enabled: true,
      enrich_attempted: false,
    };
    (mod.commands.modsListInstalled as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce({ status: 'ok', data: [unresolved] })
      .mockResolvedValueOnce({
        status: 'ok',
        // Distinguishable filename so the assertion proves the re-fetched
        // list (not the pre-backfill list) is what gets rendered.
        data: [
          {
            ...unresolved,
            filename: 'bundled-after-backfill.jar',
            name: 'bundled-after-backfill.jar',
            enrich_attempted: true,
          },
        ],
      });
    (mod.commands.modsPackOriginSummary as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'ok',
      data: { project_name: 'Parasites Reloaded', mod_shas: ['pack1'] },
    });
    render(InstalledModsView, {
      props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(mod.commands.modsEnrichPackMods).toHaveBeenCalledWith('i');
    expect(mod.commands.modsListInstalled).toHaveBeenCalledTimes(2);
    // The re-fetched list is what renders — proves `r = r2` was consumed.
    expect(screen.getByText('bundled-after-backfill.jar')).toBeTruthy();
    expect(screen.queryByText('bundled.jar')).toBeNull();
  });
});
