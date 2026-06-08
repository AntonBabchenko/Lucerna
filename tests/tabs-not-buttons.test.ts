// Regression guard: navigation tabs must NOT use .btn-* utility classes.
// Catches the B2a regression reverted in ebd27f6 where tabs were accidentally
// migrated to btn-secondary during the button-system sweep.

import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// ── SettingsModal needs real inner panels stubbed (they fire IPC on mount) ──
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // SettingsModal / CurseForgeKeyForm / StoragePanel
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsCacheSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsClearCache: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    // MainTabs / ModBrowserTab / ModBrowseView / InstalledModsView
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsInspectLocal: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        loader_mismatch: false,
        mc_mismatch: false,
        detected_loader: null,
        detected_mc: null,
      },
    }),
    modsInstallLocal: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // WorldsTab (mounted by MainTabs)
    listWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // ModpacksTab / ModpackBrowseView
    modpackSearch: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, limit: 20 },
    }),
    modpackSourceCaps: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { needs_api_key: false, supports_server_filter: true, can_export: true },
    }),
    modpackInspect: vi.fn().mockResolvedValue({
      status: 'error',
      error: { kind: 'modpack_format_unknown' },
    }),
    modpackImport: vi.fn(),
    modpackFetchToTemp: vi.fn(),
    // GeneralPanel (now the default Settings tab) calls these on mount.
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        general: { hide_to_tray_on_launch: false, theme: 'system', replay_tour_pending: false },
      },
    }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsMarkTourCompleted: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: vi.fn() }));
// MainTabs and ModpacksTab each register a window-level drag-drop listener.
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

import MainTabs from '$lib/layout/MainTabs.svelte';
import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';
import ModBrowserTab from '$lib/mods/AddonsTab.svelte';
import SettingsModal from '$lib/settings/SettingsModal.svelte';
import { settingsOpen } from '$lib/settings/state.svelte';

// ── Helper: asserts none of the .btn-* variants are present on a tab element ──
function assertNotBtnVariant(tab: Element) {
  expect(tab).not.toHaveBtnVariant('primary');
  expect(tab).not.toHaveBtnVariant('success');
  expect(tab).not.toHaveBtnVariant('danger');
  expect(tab).not.toHaveBtnVariant('secondary');
  expect(tab).not.toHaveBtnVariant('tertiary');
  expect(tab).not.toHaveBtnVariant('warning');
}

// ── SettingsModal ─────────────────────────────────────────────────────────────

afterEach(() => {
  settingsOpen.value = null;
});

describe('SettingsModal tabs are not .btn-*', () => {
  it('all 4 tabs (CurseForge, Storage, About, General) use underlined-tab style, not .btn-*', () => {
    settingsOpen.value = { tab: 'curseforge' };
    render(SettingsModal);

    const tabs = screen.getAllByRole('tab');
    expect(tabs).toHaveLength(4);
    for (const tab of tabs) {
      assertNotBtnVariant(tab);
    }
  });
});

// ── MainTabs ──────────────────────────────────────────────────────────────────

describe('MainTabs are not .btn-*', () => {
  it('all top-level tabs (Overview, Mod browser, Worlds) use underlined-tab style, not .btn-*', () => {
    render(MainTabs, { props: {} });

    const tabs = screen.getAllByRole('tab');
    expect(tabs.length).toBeGreaterThanOrEqual(3);
    for (const tab of tabs) {
      assertNotBtnVariant(tab);
    }
  });
});

// ── ModBrowserTab ─────────────────────────────────────────────────────────────

describe('Mod browser tabs are not .btn-*', () => {
  it('the content-kind switch (3) and sub-tabs (2) all use underlined-tab style, not .btn-*', () => {
    render(ModBrowserTab, { props: { instanceId: null, mcVersion: null, loader: null } });

    // 3 content-kind tabs (Mods/Resource packs/Shaders) + 2 sub-tabs (Browse/Installed).
    const tabs = screen.getAllByRole('tab');
    expect(tabs).toHaveLength(5);
    for (const tab of tabs) {
      assertNotBtnVariant(tab);
    }
  });
});

// ── ModpacksTab ───────────────────────────────────────────────────────────────

describe('Modpacks sub-tabs are not .btn-*', () => {
  it('both sub-tabs (Browse, Imported) use underlined-tab style, not .btn-*', () => {
    render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });

    // The view now renders a parent "Modpacks browser" tab above the
    // Browse | Imported sub-tabs, so there are three role="tab" elements.
    // This test guards only the underline-style sub-tabs — select them by
    // name rather than asserting a fixed total.
    const subTabs = [
      screen.getByRole('tab', { name: 'Browse' }),
      screen.getByRole('tab', { name: 'Imported' }),
    ];
    for (const tab of subTabs) {
      assertNotBtnVariant(tab);
    }
  });
});
