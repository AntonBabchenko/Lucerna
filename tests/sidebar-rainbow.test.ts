import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { rainbowFx } from '$lib/fx/rainbow-fx.svelte';
import Sidebar from '$lib/layout/Sidebar.svelte';

vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      beginMicrosoftSignin: vi.fn().mockResolvedValue({ status: 'ok', data: {} }),
    },
  };
});

function baseProps() {
  return {
    accounts: [],
    activeAccount: null,
    instances: [],
    activeInstance: null,
    onSelectAccount: vi.fn(),
    onRemoveAccount: vi.fn(),
    onOpenCosmetics: vi.fn(),
    onAddOffline: vi.fn(),
    onSelectInstance: vi.fn(),
    onOpenManage: vi.fn(),
    onOpenMods: vi.fn(),
    onOpenLogs: vi.fn(),
    onOpenModpacks: vi.fn(),
    onOpenLauncherImport: vi.fn(),
    running: null,
    installing: false,
    onPlay: vi.fn(),
    onStop: vi.fn(),
    onInstall: vi.fn(),
  };
}

afterEach(() => {
  rainbowFx.set(true); // restore default-on for other suites
});

describe('Sidebar rainbow icon', () => {
  it('applies icon-rainbow-hover to the package icon when enabled', () => {
    rainbowFx.set(true);
    const { getByTestId } = render(Sidebar, { props: baseProps() });
    const btn = getByTestId('sidebar-open-modpacks');
    const svg = btn.querySelector('svg');
    expect(svg?.classList.contains('icon-rainbow-hover')).toBe(true);
  });

  it('omits icon-rainbow-hover when disabled', () => {
    rainbowFx.set(false);
    const { getByTestId } = render(Sidebar, { props: baseProps() });
    const btn = getByTestId('sidebar-open-modpacks');
    const svg = btn.querySelector('svg');
    expect(svg?.classList.contains('icon-rainbow-hover')).toBe(false);
  });

  it('Browse modpacks button no longer carries the accent-hover override', () => {
    const { getByTestId } = render(Sidebar, { props: baseProps() });
    const btn = getByTestId('sidebar-open-modpacks');
    expect(btn.className).not.toContain('hover:bg-accent-soft');
    expect(btn.className).not.toContain('hover:border-accent');
  });

  it('Import launcher button no longer carries the accent-hover override', () => {
    const { getByTestId } = render(Sidebar, { props: baseProps() });
    const btn = getByTestId('sidebar-open-launcher-import');
    expect(btn.className).not.toContain('hover:bg-accent-soft');
    expect(btn.className).not.toContain('hover:border-accent');
  });
});
