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

// The box the clamp has to keep on screen. Menu.svelte writes top/left into an
// inline style, so the assertions need the layout's own dimensions: MENU_WIDTH
// and the three-row height estimate it clamps against.
const MENU_WIDTH = 180;
const MENU_HEIGHT = 3 * 34 + 10;

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

  // Nine of the app's inputs are type="number". This pins the wiring — an
  // unselected field offers Paste alone — not the number-specific branch:
  // nothing here sets a selection, so it would read the same on any type. That
  // the selection API is unavailable on a number input, and that both the
  // throwing and the null shape collapse to "no range", is covered directly in
  // tests/ui/edit-menu.test.ts.
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

  // Right-click near an edge — a Settings right-column field, anything low in
  // a scrollable panel — and an unclamped menu renders off-screen, putting all
  // three items out of reach. Asserted as the property (stays on screen)
  // rather than by re-deriving the formula, so a wrong clamp still fails.
  it('clamps the menu back inside the viewport near the bottom-right corner', async () => {
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    const x = window.innerWidth - 5;
    const y = window.innerHeight - 5;
    // The point is only meaningful if the raw coordinates really are outside.
    expect(x + MENU_WIDTH).toBeGreaterThan(window.innerWidth);

    await fireEvent(
      el,
      new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }),
    );

    const menu = (await screen.findByRole('menu')) as HTMLElement;
    const left = Number.parseFloat(menu.style.left);
    const top = Number.parseFloat(menu.style.top);
    expect(left).toBeGreaterThanOrEqual(0);
    expect(top).toBeGreaterThanOrEqual(0);
    expect(left + MENU_WIDTH).toBeLessThanOrEqual(window.innerWidth);
    expect(top + MENU_HEIGHT).toBeLessThanOrEqual(window.innerHeight);
  });

  // Every other Menu consumer returns focus to its trigger on close. Here the
  // trigger is the field, so backing out with Escape must not strand a
  // keyboard user on <body> in the middle of a form.
  it('returns focus to the field when the menu closes without a pick', async () => {
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const menu = await screen.findByRole('menu');
    await fireEvent.keyDown(menu, { key: 'Escape' });

    expect(screen.queryByTestId('edit-menu-copy')).toBeNull();
    expect(document.activeElement).toBe(el);
  });

  it('restores focus and the selection on the async paste path too', async () => {
    vi.mocked(commands.clipboardReadText).mockResolvedValue({
      status: 'ok',
      data: 'pasted',
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    el.setSelectionRange(1, 4);
    const exec = vi.fn().mockReturnValue(true);
    document.execCommand = exec;

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fireEvent.click(await screen.findByTestId('edit-menu-paste'));

    await waitFor(() => expect(exec).toHaveBeenCalledWith('insertText', false, 'pasted'));
    // The await is between the click and here; focus and the caret must have
    // been put back AFTER it, not before.
    expect(document.activeElement).toBe(el);
    expect(el.selectionStart).toBe(1);
    expect(el.selectionEnd).toBe(4);
  });

  // The case above cannot actually tell the two orderings apart: nothing takes
  // focus during its await, so a restore that ran too early still looks right
  // at the end. Something must hold focus while the read is in flight — in the
  // app that is Menu.svelte's own focus() on open — for the ordering to bite.
  it('puts focus back after the clipboard read, not before it', async () => {
    const thief = fieldFor('text');
    vi.mocked(commands.clipboardReadText).mockImplementation(async () => {
      thief.focus();
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
      return { status: 'ok', data: 'pasted' } as any;
    });
    render(Layout, { props: { children: emptySnippet } });
    const el = fieldFor('text');
    el.setSelectionRange(1, 4);
    const exec = vi.fn().mockReturnValue(true);
    document.execCommand = exec;

    await fireEvent(el, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fireEvent.click(await screen.findByTestId('edit-menu-paste'));

    await waitFor(() => expect(exec).toHaveBeenCalledWith('insertText', false, 'pasted'));
    expect(document.activeElement).toBe(el);
    expect(el.selectionStart).toBe(1);
    expect(el.selectionEnd).toBe(4);
  });

  // Open over A, pick Paste, then right-click B while the IPC is still in
  // flight. A's text still belongs in A (the field is captured), but the
  // restore must not yank focus off B's freshly-opened menu, which would stop
  // it answering Escape and the arrow keys.
  it('does not steal focus back when a newer menu opened during the paste', async () => {
    let resolveRead: (value: unknown) => void = () => {};
    vi.mocked(commands.clipboardReadText).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRead = resolve;
          // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
        }) as any,
    );
    render(Layout, { props: { children: emptySnippet } });
    const a = fieldFor('text');
    const b = fieldFor('text');
    const exec = vi.fn().mockReturnValue(true);
    document.execCommand = exec;

    await fireEvent(a, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fireEvent.click(await screen.findByTestId('edit-menu-paste'));
    await fireEvent(b, new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const newerMenu = await screen.findByRole('menu');

    resolveRead({ status: 'ok', data: 'pasted' });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(document.activeElement).not.toBe(a);
    expect(document.activeElement).toBe(newerMenu);
    expect(exec).not.toHaveBeenCalled();
  });
});
