import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import Select from '$lib/ui/Select.svelte';

const OPTIONS = [
  { value: 'a', label: 'Apple' },
  { value: 'b', label: 'Banana' },
  { value: 'c', label: 'Cherry' },
];

function setup(props: Record<string, unknown> = {}) {
  const onChange = vi.fn();
  const onOptionContextMenu = vi.fn();
  render(Select, {
    props: { value: 'a', options: OPTIONS, onChange, onOptionContextMenu, ...props },
  });
  const trigger = screen.getByRole('combobox');
  return { onChange, onOptionContextMenu, trigger };
}

describe('Select — a row menu from the keyboard', () => {
  it('Shift+F10 opens the active row’s menu, closes the list and keeps focus on the trigger', async () => {
    const { onOptionContextMenu, onChange, trigger } = setup();
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' }); // open on Apple
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' }); // Banana
    await fireEvent.keyDown(trigger, { key: 'F10', shiftKey: true });
    expect(onOptionContextMenu).toHaveBeenCalledTimes(1);
    const [opt, pos, how] = onOptionContextMenu.mock.calls[0];
    expect(opt.value).toBe('b');
    expect(typeof pos.x).toBe('number');
    expect(typeof pos.y).toBe('number');
    expect(how).toEqual({ byKeyboard: true });
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('the ContextMenu key does the same', async () => {
    const { onOptionContextMenu, trigger } = setup();
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    await fireEvent.keyDown(trigger, { key: 'ContextMenu' });
    expect(onOptionContextMenu.mock.calls[0][0].value).toBe('a');
    expect(onOptionContextMenu.mock.calls[0][2]).toEqual({ byKeyboard: true });
  });

  it('does nothing while the list is closed', async () => {
    const { onOptionContextMenu, trigger } = setup();
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'F10', shiftKey: true });
    expect(onOptionContextMenu).not.toHaveBeenCalled();
  });

  it('without a menu hook the key leaves the list open', async () => {
    const { trigger } = setup({ onOptionContextMenu: undefined });
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    await fireEvent.keyDown(trigger, { key: 'F10', shiftKey: true });
    expect(screen.getByRole('listbox')).toBeTruthy();
  });

  it('a right-click still opens the menu, flagged as not from the keyboard', async () => {
    const { onOptionContextMenu, trigger } = setup();
    await fireEvent.click(trigger);
    await fireEvent.contextMenu(screen.getAllByRole('option')[2], { clientX: 10, clientY: 20 });
    expect(onOptionContextMenu).toHaveBeenCalledWith(
      expect.objectContaining({ value: 'c' }),
      { x: 10, y: 20 },
      { byKeyboard: false },
    );
  });
});
