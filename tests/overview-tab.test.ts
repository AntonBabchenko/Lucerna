import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({ commands: { modpacksCheckUpdates: vi.fn() } }));

import { whatsNewState } from '$lib/changelog/whats-new.svelte';
import { hasSeen, markSeen, OVERVIEW_STEPS } from '$lib/onboarding/contextual-tours';
import { tourState } from '$lib/onboarding/state.svelte';
import { attentionCollapse } from '$lib/overview/attention-collapse.svelte';
import OverviewTab from '$lib/overview/OverviewTab.svelte';
import { serversUi } from '$lib/servers/servers-ui.svelte';

beforeEach(() => {
  attentionCollapse.reset();
  // Hygiene, not a load-bearing fix: every render below inherits
  // `installedStats.total: 18`, so the localization row — and the one-step tour
  // anchored on it — is mountable in all ~30 of them. Nothing collides with the
  // stray popover today (removing this line still leaves the file green, since
  // the first render simply burns the tour for the rest of it), but the three
  // tour tests below call `localStorage.clear()`, and there is no global
  // localStorage reset in tests/vitest.setup.ts. Re-seeding per test is what
  // keeps the file order-independent under `--sequence.shuffle`.
  markSeen('overview');
});

// Both are module-level singletons shared by every test in this file, and the
// tour tests below write to them — reset unconditionally so a failure mid-test
// can't leak "the main tour is up" or "we're in servers mode" into a neighbour.
afterEach(() => {
  tourState.active = false;
  serversUi.setMode('client');
  whatsNewState.entries = null;
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
  onOpenLocalization: () => {},
  l10nLang: 'ru_ru',
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

  it('shows the localization row once mods are installed', () => {
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst },
    });
    expect(getByTestId('overview-localization')).toBeTruthy();
  });

  it('hides the localization row when no mods are installed', () => {
    const { queryByTestId } = render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        installedStats: { total: 0, enabled: 0, disabled: 0 },
      },
    });
    expect(queryByTestId('overview-localization')).toBeNull();
  });

  it('opens localization when the row is clicked', async () => {
    let opened = 0;
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, onOpenLocalization: () => opened++ },
    });
    await fireEvent.click(getByTestId('overview-localization'));
    expect(opened).toBe(1);
  });

  // Regression coverage: measuring against the wrong language and reporting
  // a bare percentage is how the original bug hid itself (81% in the modal
  // for ru_ru, 100% on this row — because the row silently measured en_us
  // instead). The row must always say which language its number is for.
  it('names the language the translation percent was measured against', () => {
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, l10nPercent: 81, l10nLang: 'ru_ru' },
    });
    expect(getByTestId('overview-localization').textContent).toContain('ru_ru');
    expect(getByTestId('overview-localization').textContent).toContain('81%');
  });

  it('updates the row label when the shared language changes', async () => {
    const { getByTestId, rerender } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, l10nPercent: 81, l10nLang: 'ru_ru' },
    });
    expect(getByTestId('overview-localization').textContent).toContain('ru_ru');

    await rerender({
      ...baseProps,
      activeInstance: fabricInst,
      l10nPercent: 42,
      l10nLang: 'de_de',
    });
    expect(getByTestId('overview-localization').textContent).toContain('de_de');
    expect(getByTestId('overview-localization').textContent).not.toContain('ru_ru');
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

// The store the translations live in is global, so an instance drifts out of
// step without anyone touching it — a friend's import or a translation made
// elsewhere leaves this instance's pack stale, or never built at all. The badge
// is the only passive tell, so its presence has to be pinned in both
// directions: silent when there is nothing to say, loud when there is.
describe('OverviewTab localization badge', () => {
  it('shows no badge when the state is omitted or null', () => {
    const omitted = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, l10nPercent: 81 },
    });
    expect(omitted.queryByTestId('l10n-badge')).toBeNull();

    const explicitNull = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, l10nPercent: 81, l10nBadge: null },
    });
    expect(explicitNull.queryByTestId('l10n-badge')).toBeNull();
  });

  it('badges the row when the pack was never applied to this instance', () => {
    const { getByTestId } = render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        l10nPercent: 81,
        l10nBadge: 'not_applied',
      },
    });
    expect(getByTestId('l10n-badge').textContent).toContain('not applied');
  });

  it('badges the row when the applied pack has gone stale', () => {
    const { getByTestId } = render(OverviewTab, {
      props: { ...baseProps, activeInstance: fabricInst, l10nPercent: 81, l10nBadge: 'outdated' },
    });
    expect(getByTestId('l10n-badge').textContent).toContain('outdated');
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

// Overview is the DEFAULT main tab, so unlike every other contextual-tour host
// it is already mounted at startup, racing initOnboarding's two awaited IPC
// round-trips. ContextualTour's own deferral is mount-time only, so a plain
// unconditional mount here would either open on top of the main tour or defer
// once and stay inert until the user happened to switch tabs away and back.
// Hence the REACTIVE gate — and hence these tests, which pin each of its
// conjuncts to the failure it exists to prevent.
describe('OverviewTab contextual tour', () => {
  // The step names its anchor by selector, and ContextualTour.updateRect()
  // degrades a missing one SILENTLY: no spotlight, and a centred popover
  // describing a row that is nowhere on screen. Nothing else in the suite would
  // notice — the tour still mounts and still reads "Got it".
  it('renders the DOM anchor the tour step points at', () => {
    render(OverviewTab, { props: { ...baseProps, activeInstance: fabricInst } });
    const selectors = OVERVIEW_STEPS.map((s) => s.targetSelector).filter(
      (s): s is string => typeof s === 'string',
    );
    expect(selectors).toHaveLength(OVERVIEW_STEPS.length);
    for (const sel of selectors) {
      expect(document.querySelector(sel), sel).not.toBeNull();
    }
  });

  it('overview tour waits out the main tour, then fires on its completion', async () => {
    localStorage.clear(); // this test needs the tour unseen
    tourState.active = true;
    render(OverviewTab, { props: { ...baseProps, activeInstance: fabricInst } });
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();

    tourState.active = false;
    await tick();
    await tick();
    expect(screen.getByTestId('contextual-tour-popover')).toBeTruthy();
    expect(hasSeen('overview')).toBe(false); // fired, not yet finished
  });

  // In servers mode the whole client panel is class:hidden (display:none), not
  // {#if}-removed — so a tour activating in here would paint nothing, set
  // body[data-ctx-tour-active] (swallowing every Modal's Escape), and be burned
  // unseen by the first Escape the user pressed to close something else.
  it('does not fire in servers mode, where the client panel is display:none', async () => {
    localStorage.clear();
    serversUi.setMode('servers');
    render(OverviewTab, { props: { ...baseProps, activeInstance: fabricInst } });
    await tick();
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('overview')).toBe(false);
  });

  // Same gate the localization row itself renders under: no mods, no row, so
  // nothing for the single step to anchor on.
  it('does not fire when the instance has no mods, so the row is absent', async () => {
    localStorage.clear();
    render(OverviewTab, {
      props: {
        ...baseProps,
        activeInstance: fabricInst,
        installedStats: { total: 0, enabled: 0, disabled: 0 },
      },
    });
    await tick();
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    // Absence alone is too weak a claim: a regression that mounted the block
    // and immediately unmounted it would burn the tour PERMANENTLY via
    // ContextualTour's onDestroy microtask, while the query above still read
    // null. "Suppressed" has to mean "still armed for its next chance".
    expect(hasSeen('overview')).toBe(false);
  });

  // installedStats and activeInstance are separate signals fed by an async
  // refresh, so "no instance selected" can be on screen for a flush while the
  // previous instance's count is still ≥ 1 — and the placeholder branch renders
  // no localization row at all. Without this conjunct the tour would open a
  // full-screen dim and a centred popover about a row that isn't there, then
  // burn itself on "Got it".
  it('does not fire with no instance selected, where the row is absent', async () => {
    localStorage.clear();
    render(OverviewTab, { props: { ...baseProps, activeInstance: null } });
    await tick();
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('overview')).toBe(false); // suppressed, not burned — see above
  });

  // The post-update changelog offer (checkWhatsNew) and this tour both fire at
  // startup on the default tab, and the dialog it opens is z-50 against the
  // contextual dim's z-100 — so a tour left running paints its scrim OVER the
  // changelog the user just asked to read, and Modal routes their first Escape
  // to the tour instead of closing the dialog. The user clicked for the
  // changelog; the passive hint yields to it.
  it('does not fire while the changelog dialog is open', async () => {
    localStorage.clear();
    whatsNewState.entries = [{ version: '0.23.0', added: ['x'] }] as never;
    render(OverviewTab, { props: { ...baseProps, activeInstance: fabricInst } });
    await tick();
    await tick();
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('overview')).toBe(false);
  });

  // The half that is easy to get wrong. Yielding is implemented by the reactive
  // gate dropping the block, which routes through ContextualTour's onDestroy —
  // whose whole job is to burn a tour whose host went away. Burning here would
  // mean the user reads the changelog once and NEVER sees this tour, on any
  // later launch. Suppressed must keep meaning "still armed".
  it('is not burned when the changelog opens mid-tour, and fires again after it closes', async () => {
    localStorage.clear();
    render(OverviewTab, { props: { ...baseProps, activeInstance: fabricInst } });
    await tick();
    await tick();
    expect(screen.getByTestId('contextual-tour-popover')).toBeTruthy();

    whatsNewState.entries = [{ version: '0.23.0', added: ['x'] }] as never;
    await tick();
    await tick();
    await new Promise((r) => queueMicrotask(() => r(null))); // past onDestroy's microtask
    expect(screen.queryByTestId('contextual-tour-popover')).toBeNull();
    expect(hasSeen('overview')).toBe(false);

    whatsNewState.entries = null;
    await tick();
    await tick();
    expect(screen.getByTestId('contextual-tour-popover')).toBeTruthy();
  });
});
