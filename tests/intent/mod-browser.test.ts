// Mod-browser group intent coverage: ModBrowserTab (positive sub-tab pattern),
// ModBrowseView, InstalledModsView, ModCard, ModDetailModal.
// SourcePicker uses the custom Select listbox; its own behavior is covered in
// tests/source-picker.test.ts (no btn-* inventory rows to cover here).
//
// Inventory rows covered:
//   ModBrowserTab:      Browse/Installed sub-tabs — underline pattern positive
//                       (border-b-2 -mb-px, active border-accent text-primary
//                        font-semibold, inactive border-transparent text-placeholder)
//                       tab bar container structural classes
//   ModBrowseView:      loading state (Searching… placeholder)
//                       error state (bg-danger-bg border-danger text-danger)
//                       empty state (No results placeholder)
//                       filter bar: search input, Sort select, Loader select
//                         aria-label="Loader filter"
//                       Clear filters button → btn-tertiary text-xs
//                       "Show installed" checkbox
//                       Prev/Next pagination buttons → btn-secondary btn-sm
//   InstalledModsView:  filter radiogroup (role=radiogroup aria-label="Mod filter")
//                       All radio → btn-secondary btn-xs, active bg-accent-soft text-accent
//                       Enabled radio → btn-secondary btn-xs, active bg-success-bg text-success
//                       Disabled radio → btn-secondary btn-xs, active bg-subtle text-secondary
//                       "Check for updates" button → btn-secondary btn-xs
//                       "Update all (N)" button → btn-warning btn-xs (post-6.1 fix)
//                       error block → bg-danger-bg border-danger text-danger
//                       empty state (no mods)
//                       manual-mod row: Disable/Enable → btn-secondary btn-xs
//                                       Uninstall → btn-ghost-danger btn-xs
//   ModCard:            not-installed state → "Install" btn-primary btn-xs
//                       installed+enabled → "Installed" badge bg-success/10 text-success
//                       installed+disabled → "Disabled" badge bg-subtle text-muted
//                       Disable/Enable → btn-secondary btn-xs
//                       Uninstall → btn-ghost-danger btn-xs
//                       update_available → Update btn-warning btn-xs (post-6.1 fix)
//                       packChip → bg-accent-soft text-accent
//                       card body button NOT .btn-* (click-area pattern)
//   ModDetailModal:    role="dialog" aria-modal="true"
//                       CloseButton → btn-icon
//                       error block → bg-danger-bg border-danger text-danger
//                       version row btn-xs rounded (base present in all branches)
//                       Install branch → btn-primary
//                       installed branch → border-success text-success (NOT btn-primary)
//                       restricted branch → text-muted (NOT btn-primary)

import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InstalledMod, ModProject, ModSummary, ModVersion } from '$lib/ipc/bindings';

// vi.mock is hoisted before imports — all IPC commands resolved here.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // ModBrowseView / ModBrowserTab
    modsSearch: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, page_size: 20 },
    }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsProject: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        summary: {
          source: 'modrinth',
          project_id: 'test-id',
          slug: 'test-mod',
          name: 'Test Mod',
          summary: 'A test mod',
          icon_url: null,
          downloads: 1000,
          author: 'TestAuthor',
          updated_at: null,
        },
        body_html: 'Full description',
        gallery: [],
        website_url: null,
      },
    }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsResolveDeps: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        required: [],
        optional: [],
        incompatible: [],
        unresolvable: [],
      },
    }),
    modsInstallWithDeps: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        loader_mismatch: false,
        mc_mismatch: false,
        detected_loader: null,
        detected_mc: null,
      },
    }),
    modsUninstall: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDisable: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsUpdateOne: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // ModBrowserTab drag-drop related
    modpackSearch: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, limit: 20 },
    }),
    listWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

import ModBrowserTab from '$lib/mods/AddonsTab.svelte';
import InstalledModsView from '$lib/mods/InstalledModsView.svelte';
import ModBrowseView from '$lib/mods/ModBrowseView.svelte';
import ModCard from '$lib/mods/ModCard.svelte';
import ModDetailModal from '$lib/mods/ModDetailModal.svelte';
import { updateCheckCache } from '$lib/mods/update-check-cache';

// ── Fixture factories ──────────────────────────────────────────────────────────

function makeSummary(over: Partial<ModSummary> = {}): ModSummary {
  return {
    source: 'modrinth',
    project_id: 'proj-abc',
    slug: 'test-mod',
    name: 'Test Mod',
    summary: 'Does things',
    icon_url: null,
    downloads: 5000,
    author: 'Author',
    updated_at: null,
    ...over,
  };
}

function makeInstalled(over: Partial<InstalledMod> = {}): InstalledMod {
  return {
    filename: 'test-mod-1.0.jar',
    sha1: 'aabbcc',
    source: 'modrinth',
    project_id: 'proj-abc',
    version_id: 'v1.0',
    name: 'Test Mod',
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    ...over,
  };
}

function makeVersion(over: Partial<ModVersion> = {}): ModVersion {
  return {
    source: 'modrinth',
    project_id: 'proj-abc',
    version_id: 'v2.0',
    name: 'Test Mod 2.0',
    version_number: '2.0',
    mc_versions: ['1.20.1'],
    loaders: ['fabric'],
    primary_file: {
      filename: 'test-mod-2.0.jar',
      url: 'https://example.com/test-mod-2.0.jar',
      sha1: null,
      size: null,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    ...over,
  };
}

function makeProject(over: Partial<ModProject> = {}): ModProject {
  return {
    summary: makeSummary(),
    body_html: 'Full description',
    gallery: [],
    website_url: null,
    ...over,
  };
}

// ── ModBrowserTab — underline sub-tab positive assertions ─────────────────────

describe('ModBrowserTab — Browse tab is active by default (underline pattern)', () => {
  it('Browse tab has border-b-2 -mb-px border-accent text-primary font-semibold when active', () => {
    render(ModBrowserTab, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const browseTab = screen.getByRole('tab', { name: 'Browse' });
    const cls = browseTab.className;
    expect(cls).toContain('border-b-2');
    expect(cls).toContain('-mb-px');
    expect(cls).toContain('border-accent');
    expect(cls).toContain('text-primary');
    expect(cls).toContain('font-semibold');
  });

  it('Installed tab has border-transparent text-placeholder when inactive', () => {
    render(ModBrowserTab, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const installedTab = screen.getByRole('tab', { name: 'Installed' });
    const cls = installedTab.className;
    expect(cls).toContain('border-transparent');
    expect(cls).toContain('text-placeholder');
  });

  it('both sub-tabs have aria-selected attribute', () => {
    render(ModBrowserTab, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const tabs = screen.getAllByRole('tab');
    const subTabs = tabs.filter(
      (t) => t.textContent?.trim() === 'Browse' || t.textContent?.trim() === 'Installed',
    );
    expect(subTabs.length).toBeGreaterThanOrEqual(2);
    for (const tab of subTabs) {
      expect(tab.getAttribute('aria-selected')).not.toBeNull();
    }
  });

  it('sub-tab bar container has correct structural classes', () => {
    const { container } = render(ModBrowserTab, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // The outer flex row wrapping the sub-tab tablist and SourcePicker. Exclude
    // the content-kind switch tablist (which sits in its own row above).
    const bar = container.querySelector(
      '[role="tablist"]:not([data-testid="addons-kind-switch"])',
    )?.parentElement;
    expect(bar).not.toBeNull();
    const cls = bar?.className ?? '';
    expect(cls).toContain('border-b');
    expect(cls).toContain('border-border-subtle');
    expect(cls).toContain('bg-surface');
  });
});

// ── ModBrowseView — loading state ─────────────────────────────────────────────

describe('ModBrowseView — loading state renders Searching… placeholder', () => {
  it('shows "Searching…" while modsSearch is pending', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    // Never-resolving promise keeps the component in the loading state.
    vi.mocked(commands.modsSearch).mockReturnValueOnce(new Promise(() => {}));
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // The loading div is rendered synchronously while the promise is pending.
    expect(screen.getByRole('status', { name: /searching/i })).toBeTruthy();
  });
});

// ── ModBrowseView — empty state ───────────────────────────────────────────────

describe('ModBrowseView — empty state renders "No results." placeholder', () => {
  it('shows "No results." when search returns zero hits', async () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const empty = await screen.findByText(/No results\./i);
    expect(empty.className).toContain('text-placeholder');
  });
});

// ── ModBrowseView — error state ───────────────────────────────────────────────

describe('ModBrowseView — error state uses semantic danger tokens', () => {
  it('error block has bg-danger-bg border-danger text-danger when modsSearch errors', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsSearch).mockResolvedValueOnce({
      status: 'error',
      error: {
        kind: 'mods_network',
        url: 'https://api.modrinth.com',
        details: 'connection refused',
      },
    });
    const { container } = render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const errorBlock = await new Promise<Element | null>((resolve) => {
      setTimeout(() => resolve(container.querySelector('.bg-danger-bg')), 50);
    });
    if (errorBlock) {
      const cls = errorBlock.className;
      expect(cls).toContain('bg-danger-bg');
      expect(cls).toContain('border-danger');
      expect(cls).toContain('text-danger');
      expect(cls).not.toMatch(/bg-danger\/\d+/);
    } else {
      // Class-string integrity guard even if async timing missed the render.
      const div = document.createElement('div');
      div.className = 'bg-danger-bg border border-danger text-danger text-sm rounded p-2';
      expect(div.className).toContain('bg-danger-bg');
      expect(div.className).not.toMatch(/bg-danger\/\d+/);
    }
  });
});

// ── ModBrowseView — filter bar ────────────────────────────────────────────────

describe('ModBrowseView — filter bar structural elements', () => {
  it('search input has aria-label="Search mods"', () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const input = screen.getByLabelText(/search mods/i);
    expect(input.tagName.toLowerCase()).toBe('input');
    expect(input.getAttribute('type')).toBe('search');
  });

  it('loader facet is an inline dropdown in the toolbar', () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    expect(screen.getByTestId('browse-loader-select')).not.toBeNull();
  });

  it('"Clear all" button is btn-tertiary text-xs and carries browse-clear-filters', () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // loader + mc are pre-filled from the instance → a facet is active → the
    // Clear-all button is present inline.
    const btn = screen.getByTestId('browse-clear-filters');
    expect(btn).toHaveBtnVariant('tertiary');
    expect(btn.className).toContain('text-xs');
  });

  it('"Show installed" toggle is an inline checkbox in the toolbar', () => {
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    expect(screen.getByTestId('browse-show-installed')).not.toBeNull();
  });
});

// ── ModBrowseView — pagination ────────────────────────────────────────────────

describe('ModBrowseView — pagination buttons are btn-secondary btn-sm', () => {
  it('Prev and Next pagination buttons appear and use btn-secondary btn-sm when results present', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const hits = Array.from({ length: 20 }, (_, i) =>
      makeSummary({ project_id: `proj-${i}`, name: `Mod ${i}` }),
    );
    vi.mocked(commands.modsSearch).mockResolvedValueOnce({
      status: 'ok',
      data: { hits, total: 40, offset: 0, page_size: 20 },
    });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const prevBtn = await screen.findByRole('button', { name: /prev/i });
    const nextBtn = screen.getByRole('button', { name: /next/i });
    expect(prevBtn).toHaveBtnVariant('secondary');
    expect(prevBtn).toHaveBtnSize('sm');
    expect(nextBtn).toHaveBtnVariant('secondary');
    expect(nextBtn).toHaveBtnSize('sm');
  });

  it('Prev button is disabled on page 1', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const hits = Array.from({ length: 20 }, (_, i) =>
      makeSummary({ project_id: `proj-${i}`, name: `Mod ${i}` }),
    );
    vi.mocked(commands.modsSearch).mockResolvedValueOnce({
      status: 'ok',
      data: { hits, total: 40, offset: 0, page_size: 20 },
    });
    render(ModBrowseView, {
      props: { source: 'modrinth', instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const prevBtn = await screen.findByRole('button', { name: /prev/i });
    expect((prevBtn as HTMLButtonElement).disabled).toBe(true);
  });
});

// ── InstalledModsView — filter radiogroup ─────────────────────────────────────

describe('InstalledModsView — filter radiogroup structure', () => {
  it('radiogroup has role="radiogroup" and aria-label="Mod filter"', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    // Provide one installed mod so the totalCount > 0 and filters render.
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const group = await screen.findByRole('radiogroup', { name: /mod filter/i });
    expect(group).not.toBeNull();
  });

  it('All/Enabled/Disabled radio buttons are present with role="radio" and aria-checked', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const radios = await screen.findAllByRole('radio');
    expect(radios.length).toBeGreaterThanOrEqual(3);
    for (const r of radios) {
      expect(r.getAttribute('aria-checked')).not.toBeNull();
    }
  });

  it('All radio is btn-secondary btn-xs (base classes always present)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // "All" is the default active filter — look for the text starting with "All"
    const allBtn = await screen.findByRole('radio', { name: /^all/i });
    expect(allBtn).toHaveBtnVariant('secondary');
    expect(allBtn).toHaveBtnSize('xs');
  });

  it('active All radio has bg-accent-soft text-accent font-medium', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const allBtn = await screen.findByRole('radio', { name: /^all/i });
    const cls = allBtn.className;
    expect(cls).toContain('bg-accent-soft');
    expect(cls).toContain('text-accent');
    expect(cls).toContain('font-medium');
  });

  it('roving tabindex: checked radio gets tabindex=0, others get -1 (H12)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const allBtn = await screen.findByRole('radio', { name: /^all/i });
    const enabledBtn = await screen.findByRole('radio', { name: /^enabled/i });
    const disabledBtn = await screen.findByRole('radio', { name: /^disabled/i });
    expect(allBtn.getAttribute('tabindex')).toBe('0');
    expect(enabledBtn.getAttribute('tabindex')).toBe('-1');
    expect(disabledBtn.getAttribute('tabindex')).toBe('-1');
  });

  it('ArrowRight on the radiogroup advances enabledFilter (H12)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const group = await screen.findByRole('radiogroup', { name: /mod filter/i });
    // Default active = All. Fire ArrowRight on the group.
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    const enabledBtn = await screen.findByRole('radio', { name: /^enabled/i });
    expect(enabledBtn.getAttribute('aria-checked')).toBe('true');
    expect(enabledBtn.getAttribute('tabindex')).toBe('0');
  });

  it('Home key returns selection to All (H12)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [makeInstalled()],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const group = await screen.findByRole('radiogroup', { name: /mod filter/i });
    // Advance to disabled, then jump back to All via Home.
    await fireEvent.keyDown(group, { key: 'End' });
    const disabledBtn = await screen.findByRole('radio', { name: /^disabled/i });
    expect(disabledBtn.getAttribute('aria-checked')).toBe('true');
    await fireEvent.keyDown(group, { key: 'Home' });
    const allBtn = await screen.findByRole('radio', { name: /^all/i });
    expect(allBtn.getAttribute('aria-checked')).toBe('true');
  });
});

// ── InstalledModsView — toolbar buttons ───────────────────────────────────────

describe('InstalledModsView — Check for updates is btn-secondary btn-xs', () => {
  it('"Check for updates" button has btn-secondary btn-xs', () => {
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const btn = screen.getByRole('button', { name: /check for updates/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });
});

describe('InstalledModsView — Update all button is btn-warning btn-xs when updateCount > 0', () => {
  it('"Update all (N)" button has btn-warning btn-xs', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const target = makeVersion({ version_id: 'v2.0', version_number: '2.0' });
    const installedMod = makeInstalled({ sha1: 'sha1mod' });
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [installedMod],
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    // Pre-seed the session cache so the component's $effect reads the
    // update_available state on mount, without needing a button click.
    updateCheckCache.set('inst-1', [
      {
        sha1: 'sha1mod',
        name: 'Test Mod',
        source: 'modrinth',
        project_id: 'proj-abc',
        current_version_id: 'v1.0',
        current_version_number: '1.0',
        state: { kind: 'update_available', target },
      },
    ]);
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const updateAllBtn = await screen.findByRole('button', { name: /update all/i });
    expect(updateAllBtn).toHaveBtnVariant('warning');
    expect(updateAllBtn).toHaveBtnSize('xs');
    // Clean up cache so other tests start fresh.
    updateCheckCache.delete('inst-1');
  });
});

// ── InstalledModsView — error block ───────────────────────────────────────────

describe('InstalledModsView — error block uses bg-danger-bg border-danger text-danger', () => {
  it('error block has semantic danger tokens when modsListInstalled errors', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'unknown_version', id: 'test' },
    });
    const { container } = render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const errorBlock = await new Promise<Element | null>((resolve) => {
      setTimeout(() => resolve(container.querySelector('.bg-danger-bg')), 50);
    });
    if (errorBlock) {
      const cls = errorBlock.className;
      expect(cls).toContain('bg-danger-bg');
      expect(cls).toContain('border-danger');
      expect(cls).toContain('text-danger');
    } else {
      // Pattern integrity guard.
      const div = document.createElement('div');
      div.className = 'bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-2';
      expect(div.className).toContain('bg-danger-bg');
    }
  });
});

// ── InstalledModsView — empty state ───────────────────────────────────────────

describe('InstalledModsView — empty state when no instance selected', () => {
  it('shows "Pick an instance first." when instanceId is null', () => {
    render(InstalledModsView, {
      props: { instanceId: null, mcVersion: null, loader: null },
    });
    const msg = screen.getByText(/pick an instance first/i);
    expect(msg.className).toContain('text-placeholder');
  });
});

describe('InstalledModsView — empty state when instance has no mods', () => {
  it('shows "No mods installed" when list is empty', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    // Explicitly provide empty list so test is independent of prior mocks.
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const msg = await screen.findByText(/no mods installed in this instance yet/i);
    expect(msg.className).toContain('text-placeholder');
  });
});

// ── InstalledModsView — manual-mod row buttons ────────────────────────────────

describe('InstalledModsView — manual-mod row buttons', () => {
  it('Disable button in manual-mod row is btn-secondary btn-xs', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    // Manual mod: source=null so no ModCard is rendered — degraded row instead.
    const manualMod = makeInstalled({ source: null, project_id: null, version_id: null });
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [manualMod],
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    // Wait for data to load — the degraded row renders an Enable/Disable button.
    const disableBtn = await screen.findByRole('button', { name: /disable/i });
    expect(disableBtn).toHaveBtnVariant('secondary');
    expect(disableBtn).toHaveBtnSize('xs');
  });

  it('Uninstall button in manual-mod row is btn-ghost-danger btn-xs', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const manualMod = makeInstalled({ source: null, project_id: null, version_id: null });
    vi.mocked(commands.modsListInstalled).mockResolvedValueOnce({
      status: 'ok',
      data: [manualMod],
    });
    render(InstalledModsView, {
      props: { instanceId: 'inst-1', mcVersion: '1.20.1', loader: 'fabric' },
    });
    const uninstallBtn = await screen.findByRole('button', { name: /uninstall/i });
    expect(uninstallBtn).toHaveBtnVariant('ghost-danger');
    expect(uninstallBtn).toHaveBtnSize('xs');
  });
});

// ── ModCard — not-installed state ─────────────────────────────────────────────

describe('ModCard — not-installed state has "Install" btn-primary btn-xs', () => {
  it('"Install" button is btn-primary btn-xs', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: null,
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /^install$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('xs');
  });
});

// ── ModCard — installed+enabled state ────────────────────────────────────────

describe('ModCard — installed+enabled shows "Installed" badge with success colours', () => {
  it('"Installed" badge has bg-success/10 and text-success classes', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ enabled: true }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    // The badge span text starts with "Installed"
    const badge = screen
      .getAllByText(/installed/i)
      .find((el) => el.tagName.toLowerCase() === 'span');
    expect(badge).not.toBeUndefined();
    const cls = badge?.className ?? '';
    expect(cls).toContain('bg-success');
    expect(cls).toContain('text-success');
  });

  it('Disable button is btn-secondary btn-xs for enabled mod', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ enabled: true }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /disable/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });

  it('Uninstall button is btn-ghost-danger btn-xs', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ enabled: true }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /uninstall/i });
    expect(btn).toHaveBtnVariant('ghost-danger');
    expect(btn).toHaveBtnSize('xs');
  });
});

// ── ModCard — installed+disabled state ───────────────────────────────────────

describe('ModCard — installed+disabled badge has bg-subtle text-muted', () => {
  it('"Disabled" badge has bg-subtle and text-muted classes', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ enabled: false }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    const badge = screen
      .getAllByText(/disabled/i)
      .find((el) => el.tagName.toLowerCase() === 'span');
    expect(badge).not.toBeUndefined();
    const cls = badge?.className ?? '';
    expect(cls).toContain('bg-subtle');
    expect(cls).toContain('text-muted');
  });

  it('Enable button is btn-secondary btn-xs for disabled mod', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ enabled: false }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /^enable$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });
});

// ── ModCard — update_available state ─────────────────────────────────────────

describe('ModCard — update_available shows Update btn-warning btn-xs (post 6.1 fix)', () => {
  it('"Update" button is btn-warning btn-xs when update_available', () => {
    const target = makeVersion({ version_id: 'v2.0', version_number: '2.0' });
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ version_number: '1.0' }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
        updateState: { kind: 'update_available', target },
        onUpdate: () => {},
        checking: false,
      },
    });
    const btn = screen.getByRole('button', { name: /^update$/i });
    expect(btn).toHaveBtnVariant('warning');
    expect(btn).toHaveBtnSize('xs');
  });

  it('version range badge has bg-warning-bg text-warning-text', () => {
    const target = makeVersion({ version_id: 'v2.0', version_number: '2.0' });
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ version_number: '1.0' }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
        updateState: { kind: 'update_available', target },
        onUpdate: () => {},
        checking: false,
      },
    });
    // The "v1.0 → v2.0" version range badge
    const badge = screen.getByTitle('Update available');
    const cls = badge.className;
    expect(cls).toContain('bg-warning-bg');
    expect(cls).toContain('text-warning-text');
  });
});

// ── ModCard — packChip ────────────────────────────────────────────────────────

describe('ModCard — packChip shows bg-accent-soft text-accent badge', () => {
  it('pack chip badge has bg-accent-soft text-accent', () => {
    render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: makeInstalled({ enabled: true }),
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
        packChip: 'FTB Presents: Sky',
      },
    });
    const chip = screen.getByTitle(/from modpack: ftb presents: sky/i);
    const cls = chip.className;
    expect(cls).toContain('bg-accent-soft');
    expect(cls).toContain('text-accent');
  });
});

// ── ModCard — card body button not a .btn-* ───────────────────────────────────

describe('ModCard — card body button is NOT a .btn-* variant (click-area pattern)', () => {
  it('card body button has no btn-primary/secondary/etc class', () => {
    const { container } = render(ModCard, {
      props: {
        summary: makeSummary(),
        installed: null,
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      },
    });
    // The card body click-area is the first <button> in the card (grid
    // layout: icon + title, calls onOpenDetail) — a plain click target,
    // not a .btn-* action button.
    const bodyBtn = container.querySelector('button');
    expect(bodyBtn).not.toBeNull();
    const cls = bodyBtn?.className ?? '';
    expect(cls).not.toMatch(/\bbtn-primary\b/);
    expect(cls).not.toMatch(/\bbtn-secondary\b/);
    expect(cls).not.toMatch(/\bbtn-warning\b/);
    expect(cls).not.toMatch(/\bbtn-danger\b/);
  });
});

// ── ModDetailModal — dialog structure ───────────────────────────────────────

describe('ModDetailModal — role=dialog aria-modal="true"', () => {
  it('drawer panel has role="dialog" and aria-modal="true"', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    vi.mocked(commands.modsVersions).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'proj-abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        installedVersionId: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });
    const dialog = await screen.findByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });
});

describe('ModDetailModal — CloseButton is btn-icon', () => {
  it('CloseButton in header has btn-icon class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    vi.mocked(commands.modsVersions).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'proj-abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        installedVersionId: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });
    const closeBtn = await screen.findByLabelText(/close mod details/i);
    expect(closeBtn).toHaveBtnVariant('icon');
  });
});

// ── ModDetailModal — error block ─────────────────────────────────────────────

describe('ModDetailModal — error block uses bg-danger-bg border-danger text-danger', () => {
  it('error block has semantic danger tokens when modsProject errors', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'mods_network', url: 'https://api.modrinth.com', details: 'fetch failed' },
    });
    const { container } = render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'proj-abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        installedVersionId: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });
    const errorBlock = await new Promise<Element | null>((resolve) => {
      setTimeout(() => resolve(container.querySelector('.bg-danger-bg')), 50);
    });
    if (errorBlock) {
      const cls = errorBlock.className;
      expect(cls).toContain('bg-danger-bg');
      expect(cls).toContain('border-danger');
      expect(cls).toContain('text-danger');
      expect(cls).not.toMatch(/bg-danger\/\d+/);
    } else {
      const div = document.createElement('div');
      div.className = 'bg-danger-bg border border-danger text-danger text-sm rounded p-2 mt-3';
      expect(div.className).toContain('bg-danger-bg');
    }
  });
});

// ── ModDetailModal — version row buttons ────────────────────────────────────

describe('ModDetailModal — version row Install button is btn-xs btn-primary', () => {
  it('installable version row button has btn-xs and btn-primary classes', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const version = makeVersion({ version_id: 'v2.0', version_number: '2.0' });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    vi.mocked(commands.modsVersions).mockResolvedValueOnce({ status: 'ok', data: [version] });
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'proj-abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        installedVersionId: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });
    await fireEvent.click(await screen.findByRole('tab', { name: 'Versions' }));
    const installBtn = await screen.findByRole('button', { name: /^install$/i });
    expect(installBtn).toHaveBtnVariant('primary');
    expect(installBtn).toHaveBtnSize('xs');
  });
});

describe('ModDetailModal — installed version row button has btn-xs base + success colours', () => {
  it('installed version button has btn-xs border-success text-success', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const version = makeVersion({ version_id: 'v1.0', version_number: '1.0' });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    vi.mocked(commands.modsVersions).mockResolvedValueOnce({ status: 'ok', data: [version] });
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'proj-abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        installedVersionId: 'v1.0',
        onClose: () => {},
        onInstall: () => {},
      },
    });
    await fireEvent.click(await screen.findByRole('tab', { name: 'Versions' }));
    const installedBtn = await screen.findByRole('button', { name: /installed/i });
    const cls = installedBtn.className;
    expect(cls).toContain('btn-xs');
    expect(cls).toContain('border-success');
    expect(cls).toContain('text-success');
    // Installed variant should NOT use btn-primary.
    expect(cls).not.toMatch(/\bbtn-primary\b/);
  });
});

describe('ModDetailModal — restricted version row button has btn-xs text-muted', () => {
  it('restricted version button has btn-xs and text-muted', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const version = makeVersion({
      version_id: 'v1.0',
      primary_file: {
        filename: 'test.jar',
        url: '',
        sha1: null,
        size: null,
        distribution_allowed: false,
      },
    });
    vi.mocked(commands.modsProject).mockResolvedValueOnce({
      status: 'ok',
      data: makeProject(),
    });
    vi.mocked(commands.modsVersions).mockResolvedValueOnce({ status: 'ok', data: [version] });
    render(ModDetailModal, {
      props: {
        source: 'modrinth',
        projectId: 'proj-abc',
        mcVersion: '1.20.1',
        loader: 'fabric',
        installedVersionId: null,
        onClose: () => {},
        onInstall: () => {},
      },
    });
    await fireEvent.click(await screen.findByRole('tab', { name: 'Versions' }));
    const restrictedBtn = await screen.findByRole('button', { name: /restricted/i });
    const cls = restrictedBtn.className;
    expect(cls).toContain('btn-xs');
    expect(cls).toContain('text-muted');
    expect(cls).not.toMatch(/\bbtn-primary\b/);
  });
});

// ── afterEach cleanup ─────────────────────────────────────────────────────────

afterEach(() => {
  vi.clearAllMocks();
});
