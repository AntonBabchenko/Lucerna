import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({ commands: { modpacksCheckUpdates: vi.fn() } }));

import { attentionCollapse } from '$lib/overview/attention-collapse.svelte';
import OverviewTab from '$lib/overview/OverviewTab.svelte';

beforeEach(() => {
  attentionCollapse.reset();
});

const noErrors = {
  listAccounts: null,
  remove: null,
  instances: null,
  versions: null,
};
const playtimeEmpty = {
  total_seconds: 0,
  session_count: 0,
  last_session_seconds: 0,
  last_session_unix_ms: null,
};
const stats = { total: 18, enabled: 18, disabled: 0 };

const fabricInst = {
  id: 'i1',
  name: 'Skyblock',
  mc_version: '1.21.1',
  loader: 'fabric' as const,
  loader_version: '0.16.5',
  max_heap_mb: 2048,
  min_heap_mb: null,
  extra_jvm_args: '',
  created_unix_ms: null,
  ready: true,
  has_icon: false,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
  integrity: { healthy: true, checked_unix_ms: Date.now(), categories: [], problem_count: 0 },
  imported_from: null,
  created_from_server: null,
};

const baseProps = {
  installedStats: stats,
  playtime: playtimeEmpty,
  incompatibleCount: 0,
  missingModsCount: 0,
  running: false,
  installing: false,
  exited: null,
  installError: null,
  modsError: null,
  errors: noErrors,
  onManage: () => {},
  onExport: () => {},
  onOpenPackDrawer: () => {},
  onNavInstalled: () => {},
  onNavBrowse: () => {},
  onDismissError: () => {},
  onRetryError: () => {},
  onDismissInstallError: () => {},
  onDismissModsError: () => {},
  onOpenLogs: () => {},
  onOpenServers: () => {},
};

describe('OverviewTab', () => {
  it('shows the no-instance placeholder when none is selected', () => {
    const { getByText } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: null },
    });
    expect(getByText(/No instance selected/i)).toBeTruthy();
  });

  it('renders the header and no attention panel for a clean instance', () => {
    const { getByTestId, queryByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst },
    });
    expect(getByTestId('overview-instance-header')).toBeTruthy();
    expect(queryByTestId('overview-attention')).toBeNull();
    expect(queryByTestId('overview-modpack-card')).toBeNull();
  });

  it('renders the attention panel when there are incompatible mods', () => {
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, incompatibleCount: 2 },
    });
    expect(getByTestId('overview-attention-incompatible')).toBeTruthy();
  });

  it('routes the incompatible attention action to onNavInstalled', async () => {
    const onNavInstalled = vi.fn();
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, incompatibleCount: 1, onNavInstalled },
    });
    await fireEvent.click(getByTestId('overview-attention-incompatible'));
    expect(onNavInstalled).toHaveBeenCalledOnce();
  });

  it('renders the Modpack card only for pack instances', () => {
    const pack = {
      ...fabricInst,
      mrpack_name: 'All the Mods 9',
      mrpack_version: '0.2.60',
      mrpack_project_id: 'p1',
      mrpack_source: 'modrinth' as const,
    };
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: pack },
    });
    expect(getByTestId('overview-modpack-card')).toBeTruthy();
  });

  it('enables the Optimise button on a loader instance', () => {
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, onOptimise: vi.fn() },
    });
    expect((getByTestId('optimise-btn') as HTMLButtonElement).disabled).toBe(false);
  });

  it('disables the Optimise button on a vanilla instance', () => {
    const vanilla = { ...fabricInst, loader: 'vanilla' as const, loader_version: null };
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: vanilla, onOptimise: vi.fn() },
    });
    expect((getByTestId('optimise-btn') as HTMLButtonElement).disabled).toBe(true);
  });

  it('surfaces an integrity attention row for an unhealthy instance', () => {
    const unhealthy = {
      ...fabricInst,
      integrity: {
        healthy: false,
        checked_unix_ms: Date.now(),
        categories: [],
        problem_count: 3,
      },
    };
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: unhealthy },
    });
    expect(getByTestId('overview-attention-integrity')).toBeTruthy();
  });

  it('renders installError with a working dismiss button', async () => {
    const onDismissInstallError = vi.fn();
    const { getByText, getByRole } = render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        installError: 'boom',
        onDismissInstallError,
      },
    });
    expect(getByText('boom')).toBeTruthy();
    await fireEvent.click(getByRole('button', { name: 'Dismiss error' }));
    expect(onDismissInstallError).toHaveBeenCalledOnce();
  });

  it('sends each Configuration row to its own Manage field', async () => {
    const seen: (string | null | undefined)[] = [];
    const { getByTestId } = render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        onManage: (field?: string | null) => seen.push(field),
      },
    });
    await fireEvent.click(getByTestId('overview-config-header'));
    await fireEvent.click(getByTestId('overview-config-mc'));
    await fireEvent.click(getByTestId('overview-config-loader'));
    await fireEvent.click(getByTestId('overview-config-memory'));
    expect(seen).toEqual([null, 'mc', 'loader', 'memory']);
  });

  // A Configuration row's label overrides its visible text, so it has to carry
  // the current value — otherwise a screen-reader user hears "Edit Minecraft in
  // Manage" and never learns which version is set.
  it('names each Configuration row with its current value', () => {
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst },
    });
    expect(getByTestId('overview-config-mc').getAttribute('aria-label')).toContain('1.21.1');
    expect(getByTestId('overview-config-loader').getAttribute('aria-label')).toContain('0.16.5');
    expect(getByTestId('overview-config-memory').getAttribute('aria-label')).toContain('2048');
  });

  it('sends the Integrity card to the integrity section', async () => {
    const seen: (string | null | undefined)[] = [];
    const { getByTestId } = render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        onManage: (field?: string | null) => seen.push(field),
      },
    });
    await fireEvent.click(getByTestId('overview-integrity'));
    expect(seen).toEqual(['integrity']);
  });

  it('drops the secondary navigation buttons the cards used to carry', () => {
    const { queryByRole } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst },
    });
    expect(queryByRole('button', { name: /^Manage$/ })).toBeNull();
    expect(queryByRole('button', { name: /^Installed$/ })).toBeNull();
    // Anchored on purpose: the deleted buttons read exactly "Open Manage to
    // check" / "… to repair", and the anchors keep this faithful to them rather
    // than to whatever else the card happens to say.
    expect(queryByRole('button', { name: /^Open Manage to (check|repair)$/ })).toBeNull();
  });

  it('sends the mods stats row to the installed list', async () => {
    let installed = 0;
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, onNavInstalled: () => installed++ },
    });
    await fireEvent.click(getByTestId('overview-mods-stats'));
    await fireEvent.click(getByTestId('overview-mods-header'));
    expect(installed).toBe(2);
  });

  it('sends the empty mods card to the browser instead', async () => {
    let browse = 0;
    const { getByTestId } = render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        installedStats: { total: 0, enabled: 0, disabled: 0 },
        onNavBrowse: () => browse++,
      },
    });
    await fireEvent.click(getByTestId('overview-mods-empty'));
    await fireEvent.click(getByTestId('overview-mods-header'));
    expect(browse).toBe(2);
  });
});

describe('OverviewTab attention dismiss', () => {
  it('hides the panel and shows the restore triangle when collapsed', () => {
    attentionCollapse.setCollapsed('i1', true);
    const { queryByTestId, getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, incompatibleCount: 2 },
    });
    expect(queryByTestId('overview-attention')).toBeNull();
    expect(getByTestId('overview-attention-restore')).toBeTruthy();
  });

  it('restores the panel when the triangle is clicked', async () => {
    attentionCollapse.setCollapsed('i1', true);
    const { getByTestId, queryByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, incompatibleCount: 2 },
    });
    expect(queryByTestId('overview-attention')).toBeNull();
    await fireEvent.click(getByTestId('overview-attention-restore'));
    expect(getByTestId('overview-attention')).toBeTruthy();
    expect(queryByTestId('overview-attention-restore')).toBeNull();
  });

  it('collapses the panel when the dismiss X is clicked', async () => {
    const { getByTestId, queryByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, incompatibleCount: 2 },
    });
    expect(getByTestId('overview-attention')).toBeTruthy();
    await fireEvent.click(getByTestId('overview-attention-dismiss'));
    expect(queryByTestId('overview-attention')).toBeNull();
    expect(getByTestId('overview-attention-restore')).toBeTruthy();
  });
});

describe('OverviewTab version-error Reload', () => {
  it('renders a Reload button for the versions error and calls onRetryError', async () => {
    const onRetryError = vi.fn();
    render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: null,
        errors: { ...noErrors, versions: 'Network error fetching …' },
        onRetryError,
      },
    });
    const btn = screen.getByRole('button', { name: 'Reload' });
    await fireEvent.click(btn);
    expect(onRetryError).toHaveBeenCalledWith('versions');
  });

  it('does not render a Reload button for a non-retryable error (instances)', () => {
    render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: null,
        errors: { ...noErrors, instances: 'cannot list instances' },
        onRetryError: () => {},
      },
    });
    expect(screen.queryByRole('button', { name: 'Reload' })).toBeNull();
  });

  it('disables the Reload button while a retry is in flight', () => {
    render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: null,
        errors: { ...noErrors, versions: 'Network error fetching …' },
        versionsRetrying: true,
      },
    });
    expect(screen.getByRole('button', { name: 'Reload' }).hasAttribute('disabled')).toBe(true);
  });
});
