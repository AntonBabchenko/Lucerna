import { render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import CompatWarningDialog from '../src/lib/mods/CompatWarningDialog.svelte';
import ModDropzone from '../src/lib/mods/ModDropzone.svelte';

// Vitest hoists `vi.mock(...)` above the imports regardless of source
// position. ModDropzone calls `getCurrentWebview()` in `onMount` and
// `openFile` (plugin-dialog) in `clickPicker` — both need stubbing under
// happy-dom, which has no Tauri runtime.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue([]),
}));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (_cb: unknown) => Promise.resolve(() => {}),
  }),
}));

describe('ModDropzone', () => {
  test('enabled dropzone invites a drop', () => {
    render(ModDropzone, { disabled: false, onPicked: () => {} });
    expect(screen.getByText(/drop a mod .jar/i)).toBeTruthy();
  });

  test('disabled dropzone explains why', () => {
    render(ModDropzone, { disabled: true, onPicked: () => {} });
    expect(screen.getByText(/select a non-vanilla instance/i)).toBeTruthy();
  });
});

describe('CompatWarningDialog', () => {
  test('lists each mismatched jar with its reason', () => {
    render(CompatWarningDialog, {
      rows: [{ filename: 'sodium.jar', reason: 'looks like a Fabric mod, instance is forge' }],
      onConfirm: () => {},
      onCancel: () => {},
    });
    expect(screen.getByText('sodium.jar')).toBeTruthy();
    expect(screen.getByText(/looks like a Fabric mod/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: /install anyway/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /skip these/i })).toBeTruthy();
  });
});
