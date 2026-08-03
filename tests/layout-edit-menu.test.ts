import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { clipboardReadText: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import Layout from '../src/routes/+layout.svelte';

// The layout takes a required `children` Snippet prop. These cases exercise
// the window-level contextmenu handler and the menu it opens, never the body,
// so pass an empty render-nothing snippet via a type cast — the same shape
// tests/intent/browser-feel.test.ts uses.
const emptySnippet = (() => null) as unknown as never;

function fieldFor(type: string): HTMLInputElement {
  const el = document.createElement('input');
  el.type = type;
  el.value = 'hello';
  document.body.appendChild(el);
  return el;
}

afterEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = '';
});

describe('right-click edit menu', () => {
  it('opens over a text field and suppresses the native menu', async () => {
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    el.setSelectionRange(0, 5);

    const evt = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    await fireEvent(el, evt);

    expect(evt.defaultPrevented).toBe(true);
    expect(await screen.findByTestId('edit-menu-copy')).toBeTruthy();
    expect(screen.getByTestId('edit-menu-cut')).toBeTruthy();
    expect(screen.getByTestId('edit-menu-paste')).toBeTruthy();
  });

  it('offers no menu on a control with no text, but still blocks the native one', async () => {
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('checkbox');

    const evt = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    await fireEvent(el, evt);

    expect(evt.defaultPrevented).toBe(true);
    expect(screen.queryByTestId('edit-menu-copy')).toBeNull();
  });

  // Nine of the app's inputs are type="number", where the selection API is
  // unavailable. Copy and Cut have nothing to act on; Paste still does.
  it('disables copy and cut on a number field but keeps paste', async () => {
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('number');
    el.value = '42';

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));

    const copy = (await screen.findByTestId('edit-menu-copy')) as HTMLButtonElement;
    expect(copy.disabled).toBe(true);
    expect((screen.getByTestId('edit-menu-cut') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('edit-menu-paste') as HTMLButtonElement).disabled).toBe(false);
  });

  // The whole design turns on this: Menu.svelte focuses itself on open, and
  // document.execCommand acts on the FOCUSED element. Without putting focus
  // and the caret back, every item is a silent no-op.
  it('restores focus and the selection to the field before running the command', async () => {
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    el.setSelectionRange(1, 4);
    const exec = vi.fn().mockReturnValue(true);
    document.execCommand = exec;

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fireEvent.click(await screen.findByTestId('edit-menu-copy'));

    await waitFor(() => expect(exec).toHaveBeenCalledWith('copy'));
    expect(document.activeElement).toBe(el);
    expect(el.selectionStart).toBe(1);
    expect(el.selectionEnd).toBe(4);
  });

  it('pastes what the clipboard command returns', async () => {
    vi.mocked(commands.clipboardReadText).mockResolvedValue({
      status: 'ok',
      data: 'pasted',
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    const exec = vi.fn().mockReturnValue(true);
    document.execCommand = exec;

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fireEvent.click(await screen.findByTestId('edit-menu-paste'));

    await waitFor(() => expect(exec).toHaveBeenCalledWith('insertText', false, 'pasted'));
  });

  it('does nothing when the clipboard read fails', async () => {
    vi.mocked(commands.clipboardReadText).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<clipboard>', details: 'nope' },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    const exec = vi.fn().mockReturnValue(true);
    document.execCommand = exec;

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fireEvent.click(await screen.findByTestId('edit-menu-paste'));

    await waitFor(() => expect(commands.clipboardReadText).toHaveBeenCalled());
    expect(exec).not.toHaveBeenCalled();
  });
});
