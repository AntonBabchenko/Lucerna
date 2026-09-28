import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';

// The guarded mod operations (mod-ops) ask Cancel · «Only this one» · «Disable all N»: an
// alternative between Cancel and the confirm. Both actions mutate asynchronously, so each has its
// own busy flag — one `busy` cannot say which button spins.
const base = () => ({
  title: 'Disable Sodium?',
  bodyText: 'Indium will not load without it.',
  confirmLabel: 'Disable all 2',
  onCancel: vi.fn(),
  onConfirm: vi.fn(),
});
const withSecondary = () => ({ ...base(), secondaryLabel: 'Only this one', onSecondary: vi.fn() });
// Regex names: a busy BusyButton's accessible name also carries its spinner's label.
const btn = (name: string) =>
  screen.getByRole('button', { name: new RegExp(name) }) as HTMLButtonElement;
const labels = () => screen.getAllByRole('button').map((b) => b.textContent?.trim());

describe('ConfirmDialog — optional secondary action', () => {
  it('renders no third button unless the caller asks for one', () => {
    render(ConfirmDialog, { props: base() });
    expect(labels()).toEqual(['Cancel', 'Disable all 2']);
  });

  it('renders no third button for a label without a handler', () => {
    render(ConfirmDialog, { props: { ...base(), secondaryLabel: 'Only this one' } });
    expect(labels()).toEqual(['Cancel', 'Disable all 2']);
  });

  it('sits between Cancel and the confirm and runs onSecondary, not onConfirm', async () => {
    const props = withSecondary();
    render(ConfirmDialog, { props });
    expect(labels()).toEqual(['Cancel', 'Only this one', 'Disable all 2']);
    await fireEvent.click(btn('Only this one'));
    expect(props.onSecondary).toHaveBeenCalledOnce();
    expect(props.onConfirm).not.toHaveBeenCalled();
  });

  it('while the secondary runs only it spins, and nothing can be pressed or dismissed', async () => {
    const props = { ...withSecondary(), secondaryBusy: true };
    render(ConfirmDialog, { props });
    expect(btn('Only this one').getAttribute('aria-busy')).toBe('true');
    expect(btn('Disable all 2').getAttribute('aria-busy')).toBe('false');
    for (const name of ['Cancel', 'Only this one', 'Disable all 2'])
      expect(btn(name).disabled).toBe(true);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(props.onCancel).not.toHaveBeenCalled();
  });

  it('while the confirm runs the secondary is locked too', async () => {
    const props = { ...withSecondary(), busy: true };
    render(ConfirmDialog, { props });
    expect(btn('Disable all 2').getAttribute('aria-busy')).toBe('true');
    expect(btn('Only this one').disabled).toBe(true);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(props.onCancel).not.toHaveBeenCalled();
  });

  it('parks focus on the dialog body when the pressed action turns busy (DESIGN.md §8)', async () => {
    const props = withSecondary();
    const { rerender } = render(ConfirmDialog, { props });
    btn('Only this one').focus();
    await rerender({ ...props, secondaryBusy: true });
    expect(document.activeElement?.id).toMatch(/^confirm-body-/);
  });
});
