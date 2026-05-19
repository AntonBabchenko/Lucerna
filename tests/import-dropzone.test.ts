import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { describe, expect, it, vi } from 'vitest';

// Capture the drag-drop handler at module-eval time so tests can drive it.
const dragHandlers = vi.hoisted(() => ({
  handler: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue('C:\\packs\\pack.mrpack'),
}));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (cb: (event: { payload: unknown }) => void) => {
      dragHandlers.handler = cb;
      return Promise.resolve(() => {
        dragHandlers.handler = null;
      });
    },
  }),
}));

import ImportDropzone from '$lib/modpacks/ImportDropzone.svelte';

async function flush() {
  // Allow onMount + listener registration to complete.
  await tick();
  await new Promise((r) => setTimeout(r, 0));
  await tick();
}

describe('ImportDropzone', () => {
  it('renders the dropzone', () => {
    const { getByTestId } = render(ImportDropzone, {
      props: { onPicked: () => {}, onError: () => {} },
    });
    expect(getByTestId('import-dropzone')).toBeTruthy();
  });

  it('calls onPicked when the user picks a file via the native dialog', async () => {
    const onPicked = vi.fn();
    const { getByTestId } = render(ImportDropzone, {
      props: { onPicked, onError: () => {} },
    });
    await fireEvent.click(getByTestId('import-dropzone'));
    // open() is async; let microtasks flush so the .then callback runs.
    await new Promise((r) => setTimeout(r, 0));
    expect(onPicked).toHaveBeenCalledWith('C:\\packs\\pack.mrpack');
  });

  it('accepts a dragged .mrpack file', async () => {
    const onPicked = vi.fn();
    render(ImportDropzone, {
      props: { onPicked, onError: () => {} },
    });
    await flush();
    dragHandlers.handler?.({
      payload: { type: 'drop', paths: ['C:\\downloads\\modpack.mrpack'] },
    });
    expect(onPicked).toHaveBeenCalledWith('C:\\downloads\\modpack.mrpack');
  });

  it('accepts a dragged .zip file', async () => {
    const onPicked = vi.fn();
    render(ImportDropzone, {
      props: { onPicked, onError: () => {} },
    });
    await flush();
    dragHandlers.handler?.({
      payload: { type: 'drop', paths: ['C:\\downloads\\pack.zip'] },
    });
    expect(onPicked).toHaveBeenCalledWith('C:\\downloads\\pack.zip');
  });

  it('rejects a dragged file with the wrong extension', async () => {
    const onPicked = vi.fn();
    const onError = vi.fn();
    render(ImportDropzone, {
      props: { onPicked, onError },
    });
    await flush();
    dragHandlers.handler?.({
      payload: { type: 'drop', paths: ['C:\\downloads\\readme.txt'] },
    });
    expect(onPicked).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith('Drop a .mrpack or .zip file.');
  });
});
