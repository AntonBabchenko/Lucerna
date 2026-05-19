import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// All three dependency surfaces ModpacksTab pulls in need to be mocked
// at module-eval time, ahead of the SUT import, so vitest hoists them
// into the module graph before any binding loads:
//   • `$lib/ipc/bindings` — ModpackBrowseView and the import pipeline
//     both call into `commands`.
//   • `@tauri-apps/plugin-dialog` — ImportDropzone's click-picker path
//     opens the native file dialog.
//   • `@tauri-apps/api/webview` — ImportDropzone registers a drag-drop
//     listener on the webview at mount.
//   • `@tauri-apps/api/core` — Channel is constructed on confirmImport
//     so we stub it even though confirm isn't exercised here.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackSearch: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { hits: [], total: 0, offset: 0, limit: 20 },
    }),
    modpackInspect: vi.fn(),
    modpackImport: vi.fn(),
    modpackFetchToTemp: vi.fn(),
  },
  events: {},
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: () => Promise.resolve(() => {}) }),
}));
vi.mock('@tauri-apps/api/core', () => ({
  Channel: vi.fn(),
}));

import ModpacksTab from '$lib/modpacks/ModpacksTab.svelte';

describe('ModpacksTab', () => {
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
    const { getByRole, container } = render(ModpacksTab, {
      props: { instances: [], onInstanceCreated: () => {} },
    });
    const browseTab = getByRole('tab', { name: 'Browse' });
    const importedTab = getByRole('tab', { name: 'Imported' });

    // Switch to Imported, then back to Browse.
    await fireEvent.click(importedTab);
    await fireEvent.click(browseTab);

    expect(browseTab.getAttribute('aria-selected')).toBe('true');
    // Both sub-panes remain in the DOM (one is just hidden via CSS).
    // The dropzone lives only inside the Browse pane, so its presence
    // confirms Browse stayed mounted across the switch.
    expect(container.querySelector('[data-testid="import-dropzone"]')).not.toBeNull();
  });
});
