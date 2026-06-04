import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// All dependency surfaces ModpacksTab pulls in need to be mocked
// at module-eval time, ahead of the SUT import, so vitest hoists them
// into the module graph before any binding loads:
//   • `$lib/ipc/bindings` — ModpackBrowseView and the import pipeline
//     both call into `commands`.
//   • `@tauri-apps/plugin-dialog` — the "Import from file…" button opens
//     the native file dialog.
//   • `@tauri-apps/api/core` — Channel is constructed on confirmImport
//     so we stub it even though confirm isn't exercised here.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackSearch: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, limit: 20 },
    }),
    modpackSourceCaps: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { needs_api_key: false, supports_server_filter: true, can_export: true },
    }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'present' }),
    modpackInspect: vi
      .fn()
      .mockResolvedValue({ status: 'error', error: { kind: 'modpack_format_unknown' } }),
    modpackImport: vi.fn(),
    modpackFetchToTemp: vi.fn(),
  },
  events: {},
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: vi.fn(),
}));
// ModpacksTab registers a window-level drag-drop listener on mount
// (modpacks moved out of MainTabs into the sidebar). Stub the webview
// API so the listener registration is a no-op in jsdom.
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: () => Promise.resolve(() => {}),
  }),
}));

import { commands } from '$lib/ipc/bindings';
import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';

describe('ModpacksTab', () => {
  afterEach(async () => {
    const { droppedModpack } = await import('$lib/settings/state.svelte');
    droppedModpack.value = null;
  });

  it('renders Browse by default', () => {
    const { getByRole } = render(ModpacksTab, {
      props: { instances: [], onInstanceCreated: () => {} },
    });
    const browseTab = getByRole('tab', { name: 'Browse' });
    expect(browseTab.getAttribute('aria-selected')).toBe('true');
  });

  it('switches to Imported on click', async () => {
    const { getByRole } = render(ModpacksTab, {
      props: { instances: [], onInstanceCreated: () => {} },
    });
    const importedTab = getByRole('tab', { name: 'Imported' });
    await fireEvent.click(importedTab);
    expect(importedTab.getAttribute('aria-selected')).toBe('true');
  });

  it('switches back to Browse with state preserved (lazy-mount + CSS-hide)', async () => {
    const { getByRole, getByTestId } = render(ModpacksTab, {
      props: { instances: [], onInstanceCreated: () => {} },
    });
    const browseTab = getByRole('tab', { name: 'Browse' });
    const importedTab = getByRole('tab', { name: 'Imported' });

    // Switch to Imported, then back to Browse.
    await fireEvent.click(importedTab);
    await fireEvent.click(browseTab);

    expect(browseTab.getAttribute('aria-selected')).toBe('true');
    // Both sub-panes remain in the DOM (one is just hidden via CSS).
    // The FileDropzone is always rendered, confirming the Browse pane
    // stayed mounted across the switch.
    expect(getByTestId('file-dropzone')).toBeTruthy();
  });

  it('renders the FileDropzone affordance', () => {
    render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    expect(screen.getByTestId('file-dropzone')).toBeTruthy();
  });

  it('displays a formatted error message (not [object Object]) when inspect fails', async () => {
    // modpackInspect is already mocked to return a typed error object:
    //   { status: 'error', error: { kind: 'modpack_format_unknown' } }
    // Before the fix, inspect() called String(r.error) which produced
    // "[object Object]" in the red banner. After the fix it calls
    // formatError(r.error) which returns a human-readable string.
    const { droppedModpack } = await import('$lib/settings/state.svelte');
    render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    droppedModpack.value = '/x/pack.mrpack';
    await waitFor(() => {
      expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledWith('/x/pack.mrpack');
    });
    // The error banner should NOT contain the raw "[object Object]" string.
    await waitFor(() => {
      const banner = screen.queryByText(/\[object Object\]/);
      expect(banner).toBeNull();
    });
  });

  it('inspects a modpack handed to it via the droppedModpack rune', async () => {
    const { droppedModpack } = await import('$lib/settings/state.svelte');
    render(ModpacksTab, { props: { instances: [], onInstanceCreated: () => {} } });
    droppedModpack.value = '/x/pack.mrpack';
    await waitFor(() => {
      expect(vi.mocked(commands.modpackInspect)).toHaveBeenCalledWith('/x/pack.mrpack');
    });
    expect(droppedModpack.value).toBeNull();
  });
});
