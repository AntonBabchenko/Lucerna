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
    render(InstalledModsView, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
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
    render(InstalledModsView, { props: { instanceId: 'i', mcVersion: '1.20.1', loader: 'fabric' } });
    await new Promise((r) => setTimeout(r, 0));
    const buttons = screen.getAllByRole('button', { name: 'Uninstall' });
    await fireEvent.click(buttons[0]!);
    expect(mod.commands.modsUninstall).toHaveBeenCalledWith('i', 'abc');
  });

  it('shows empty state when no instance is selected', () => {
    render(InstalledModsView, {
      props: { instanceId: null, mcVersion: null, loader: null },
    });
    expect(screen.getByText(/Pick an instance first/)).toBeTruthy();
  });
});
