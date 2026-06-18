import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import PlayWithWorlds from '$lib/layout/PlayWithWorlds.svelte';

function worlds(names: string[]) {
  return names.map((folder_name, i) => ({ folder_name, modified_unix_ms: 1000 - i }));
}
function base(over: Record<string, unknown> = {}) {
  return {
    worlds: worlds(['Alpha', 'Beta']),
    onPlay: vi.fn(),
    onQuickPlayWorld: vi.fn(),
    menuEnabled: true,
    label: 'Play',
    ...over,
  };
}

afterEach(() => vi.useRealTimers());

describe('PlayWithWorlds', () => {
  it('clicking the Play button calls onPlay, not a world launch', async () => {
    const props = base();
    render(PlayWithWorlds, { props });
    await fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(props.onPlay).toHaveBeenCalledTimes(1);
    expect(props.onQuickPlayWorld).not.toHaveBeenCalled();
  });

  it('does NOT open the menu when menuEnabled is false', async () => {
    vi.useFakeTimers();
    render(PlayWithWorlds, { props: base({ menuEnabled: false }) });
    const wrap = screen.getByRole('button', { name: 'Play' }).parentElement as HTMLElement;
    await fireEvent.mouseEnter(wrap);
    await vi.advanceTimersByTimeAsync(300);
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('does NOT open the menu when there are no worlds', async () => {
    vi.useFakeTimers();
    render(PlayWithWorlds, { props: base({ worlds: [] }) });
    const wrap = screen.getByRole('button', { name: 'Play' }).parentElement as HTMLElement;
    await fireEvent.mouseEnter(wrap);
    await vi.advanceTimersByTimeAsync(300);
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('opens on hover after the delay and lists worlds in given order', async () => {
    vi.useFakeTimers();
    render(PlayWithWorlds, { props: base() });
    const wrap = screen.getByRole('button', { name: 'Play' }).parentElement as HTMLElement;
    await fireEvent.mouseEnter(wrap);
    await vi.advanceTimersByTimeAsync(200);
    const items = screen.getAllByRole('menuitem');
    expect(items.map((b) => b.textContent?.trim())).toEqual(['Alpha', 'Beta']);
  });

  it('clicking a world calls onQuickPlayWorld and closes the menu', async () => {
    vi.useFakeTimers();
    const props = base();
    render(PlayWithWorlds, { props });
    const wrap = screen.getByRole('button', { name: 'Play' }).parentElement as HTMLElement;
    await fireEvent.mouseEnter(wrap);
    await vi.advanceTimersByTimeAsync(200);
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Beta' }));
    expect(props.onQuickPlayWorld).toHaveBeenCalledWith('Beta');
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('ArrowDown opens the menu for keyboard users', async () => {
    render(PlayWithWorlds, { props: base() });
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Play' }), { key: 'ArrowDown' });
    expect(screen.getByTestId('play-worlds-menu')).toBeTruthy();
    expect(screen.getByRole('menuitem', { name: 'Alpha' })).toBeTruthy();
  });

  it('Escape on an item closes the menu', async () => {
    render(PlayWithWorlds, { props: base() });
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Play' }), { key: 'ArrowDown' });
    const item = screen.getByRole('menuitem', { name: 'Alpha' });
    await fireEvent.keyDown(item, { key: 'Escape' });
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('ArrowUp opens the menu', async () => {
    render(PlayWithWorlds, { props: base() });
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Play' }), { key: 'ArrowUp' });
    expect(screen.getByTestId('play-worlds-menu')).toBeTruthy();
  });

  it('Enter on an item launches that world and closes the menu', async () => {
    const props = base();
    render(PlayWithWorlds, { props });
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Play' }), { key: 'ArrowDown' });
    const item = screen.getByRole('menuitem', { name: 'Beta' });
    await fireEvent.keyDown(item, { key: 'Enter' });
    expect(props.onQuickPlayWorld).toHaveBeenCalledWith('Beta');
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('a pointer-down outside the control closes the menu', async () => {
    render(PlayWithWorlds, { props: base() });
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Play' }), { key: 'ArrowDown' });
    expect(screen.getByTestId('play-worlds-menu')).toBeTruthy();
    await fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('moving the mouse out of the control closes the menu', async () => {
    vi.useFakeTimers();
    render(PlayWithWorlds, { props: base() });
    const wrap = screen.getByRole('button', { name: 'Play' }).parentElement as HTMLElement;
    await fireEvent.mouseEnter(wrap);
    await vi.advanceTimersByTimeAsync(200);
    expect(screen.getByTestId('play-worlds-menu')).toBeTruthy();
    await fireEvent.mouseLeave(wrap);
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });

  it('shows a chevron affordance only when the menu can open', async () => {
    const { container, rerender } = render(PlayWithWorlds, { props: base() });
    expect(container.querySelector('.lucide-chevron-down')).not.toBeNull();
    // No worlds → nothing to drop down → no chevron hint.
    await rerender(base({ worlds: [] }));
    expect(container.querySelector('.lucide-chevron-down')).toBeNull();
  });

  it('closes when the instance becomes ineligible while open', async () => {
    const { rerender } = render(PlayWithWorlds, { props: base() });
    await fireEvent.keyDown(screen.getByRole('button', { name: 'Play' }), { key: 'ArrowDown' });
    expect(screen.getByTestId('play-worlds-menu')).toBeTruthy();
    // e.g. the game launched → page flips menuEnabled to false.
    await rerender(base({ menuEnabled: false }));
    expect(screen.queryByTestId('play-worlds-menu')).toBeNull();
  });
});
