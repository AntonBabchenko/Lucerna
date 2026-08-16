import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { type Account, commands, type InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';
import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';
import { countPillClass } from '$lib/ui/cards/CountPill.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

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

const sampleAccount: Account = {
  id: 'a1',
  kind: 'offline',
  name: 'Tester',
  uuid: '00000000-0000-0000-0000-000000000000',
  expires_at: null,
};
const sampleInstance: InstanceWithStatus = {
  id: 'i1',
  name: 'Default',
  mc_version: '1.20.1',
  loader: 'vanilla',
  loader_version: null,
  ready: true,
  has_icon: false,
  max_heap_mb: 4096,
  min_heap_mb: null,
  extra_jvm_args: '',
  created_unix_ms: null,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
  integrity: null,
  imported_from: null,
  created_from_server: null,
};

describe('Sidebar', () => {
  it('renders Lucerna title', () => {
    const { getByText } = render(Sidebar, {
      props: {
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
      },
    });
    expect(getByText('Lucerna')).toBeTruthy();
  });

  it('lists accounts and emits select on change', async () => {
    const onSelectAccount = vi.fn();
    const { getByRole } = render(Sidebar, {
      props: {
        accounts: [sampleAccount],
        activeAccount: sampleAccount,
        instances: [sampleInstance],
        activeInstance: sampleInstance,
        onSelectAccount,
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
      },
    });
    const trigger = getByRole('combobox', { name: 'Account' });
    expect(trigger.textContent).toMatch(/Tester/);
  });

  it('renders the Browse modpacks button at the sidebar level', () => {
    const { getByTestId } = render(Sidebar, {
      props: {
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
      },
    });
    expect(getByTestId('sidebar-open-modpacks')).toBeTruthy();
  });

  it('renders the Import from launcher button', () => {
    const { getByTestId } = render(Sidebar, {
      props: {
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
      },
    });
    expect(getByTestId('sidebar-open-launcher-import')).toBeTruthy();
  });

  it('shows an icon on the Settings button', () => {
    render(Sidebar, {
      props: {
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
      },
    });
    // The Settings button carries a visible label, so its accessible name is
    // "Settings" (the gear icon is decorative/aria-hidden). Assert the icon is
    // actually present — it should line up with the icon-bearing Logs button.
    const settingsBtn = screen.getByRole('button', { name: 'Settings' });
    expect(settingsBtn.querySelector('svg')).not.toBeNull();
  });

  it('does NOT render the deferred-MS italic text', () => {
    render(Sidebar, {
      props: {
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
      },
    });
    expect(screen.queryByText(/Microsoft account login deferred/i)).toBeNull();
  });

  it('shows (offline) suffix for offline accounts in the dropdown', () => {
    render(Sidebar, {
      props: {
        accounts: [sampleAccount],
        activeAccount: sampleAccount,
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
      },
    });
    expect(screen.getByText('Tester (offline)')).toBeTruthy();
  });

  it('shows (Microsoft) suffix for microsoft accounts in the dropdown', () => {
    const msAccount: Account = {
      id: 'ms1',
      kind: 'microsoft',
      name: 'MSPlayer',
      uuid: '11111111-0000-0000-0000-000000000000',
      expires_at: 9999999999,
    };
    render(Sidebar, {
      props: {
        accounts: [msAccount],
        activeAccount: msAccount,
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
      },
    });
    expect(screen.getByText('MSPlayer (Microsoft)')).toBeTruthy();
  });

  it('renders the Sign in with Microsoft button', () => {
    render(Sidebar, {
      props: {
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
      },
    });
    expect(screen.getByRole('button', { name: /sign in with microsoft/i })).toBeTruthy();
  });

  it('renders the compact toggle and emits on click', async () => {
    const onToggleCompact = vi.fn();
    const { getByLabelText } = render(Sidebar, {
      props: {
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
        compact: false,
        onToggleCompact,
      },
    });
    const toggle = getByLabelText('Collapse to mini mode');
    toggle.click();
    expect(onToggleCompact).toHaveBeenCalledTimes(1);
  });
});

describe('Sidebar modpack-update badge', () => {
  const baseProps = {
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

  // `updateCount` is derived from the store's map, so the count is driven the
  // way the app drives it — a sweep — rather than assigned.
  async function seedUpdates(ids: string[]) {
    vi.spyOn(commands, 'modpacksCheckUpdates').mockResolvedValue({
      status: 'ok',
      data: ids.map((instance_id) => ({
        instance_id,
        status: {
          kind: 'update_available' as const,
          entry: {
            id: 'v2',
            name: 'P',
            version_number: '1.5.0',
            game_versions: [],
            loaders: [],
            date_published: '',
          },
        },
      })),
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    await modpackUpdates.sweep(ids, { force: true });
  }

  afterEach(() => {
    modpackUpdates.reset();
    hideTooltip();
  });

  it('the modpack-update badge is the shared CountPill, not a local recipe', async () => {
    await seedUpdates(['a', 'b', 'c']);
    render(Sidebar, { props: baseProps });
    const badge = await screen.findByTestId('sidebar-modpack-updates-badge');

    // Every class the primitive owns must be present — this is what fails if a
    // future edit re-inlines a fourth variant of the recipe.
    for (const cls of countPillClass('md').split(' ')) {
      expect(badge.classList.contains(cls), `missing ${cls}`).toBe(true);
    }
    // Positioning stays with the call site.
    expect(badge.classList.contains('ml-1')).toBe(true);
    expect(badge.textContent).toContain('3');

    // §5: the label is the tooltip layer's, not a native title.
    expect(badge.getAttribute('title')).toBeNull();
    revealTooltip(badge);
    expect(tooltipState.text).toBe('3 modpack updates available');
  });
});
